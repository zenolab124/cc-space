//! ccimg 自定义协议 —— 历史区图片按需取。
//!
//! 背景：JSONL 里 image block 的 base64 原文动辄几百 KB，若随 records 穿透 IPC
//! 会撑爆 JS 堆。方案：解析时把 base64 从 records 剥离（ImageSource 丢弃 data），
//! 前端用 ccimg:// 协议 URL 按需向本 handler 取二进制。
//!
//! 协议 URL 格式：
//!   `ccimg://localhost/{project_id}/{session_id}/{record_uuid}/{img_index}`
//! 子 Agent 会话追加 query `?agent={agent_id}`，定位到 `subagents/agent-<id>.jsonl`。
//! project_id 已 URL-encode（目录名含 `-`），path 段取用前需 percent-decode。
//!
//! handler 流程：解析 URL → 白名单校验（防目录穿越）→ 定位 JSONL → 逐行找
//! record_uuid → 单行反序列化 → 深度优先取第 img_index 个 image 的 base64 →
//! decode 为二进制 → http::Response<Vec<u8>>。带 64MB 字节上限的 LRU 缓存。

use std::io::{BufRead, BufReader};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use lru::LruCache;
use serde_json::Value;
use tauri::http::{Request, Response, StatusCode};
use tauri::UriSchemeResponder;

/// LRU 缓存字节上限：64MB。命中键为 (jsonl 路径, record_uuid, img_index)。
const CACHE_MAX_BYTES: usize = 64 * 1024 * 1024;

/// 解码后的一张图，供缓存与响应复用。
#[derive(Clone)]
struct CachedImage {
    media_type: String,
    bytes: Vec<u8>,
}

/// 按字节数计容量的 LRU：容量给一个很大的条目数上限，真正的淘汰靠字节水位手动驱动。
struct ByteLru {
    inner: LruCache<String, CachedImage>,
    current_bytes: usize,
}

impl ByteLru {
    fn new() -> Self {
        // 条目数上限给足够大（协议图片单张通常 KB~MB 级），字节水位才是真约束。
        ByteLru {
            inner: LruCache::new(NonZeroUsize::new(4096).unwrap()),
            current_bytes: 0,
        }
    }

    fn get(&mut self, key: &str) -> Option<CachedImage> {
        self.inner.get(key).cloned()
    }

    fn put(&mut self, key: String, img: CachedImage) {
        let size = img.bytes.len();
        // 单张就超上限：不缓存，直接返回（避免反复清空缓存）。
        if size > CACHE_MAX_BYTES {
            return;
        }
        // 若 key 已存在，先扣掉旧条目的字节数。
        if let Some(old) = self.inner.pop(&key) {
            self.current_bytes = self.current_bytes.saturating_sub(old.bytes.len());
        }
        // 腾出空间：按 LRU 顺序淘汰最久未用，直到能容纳新条目。
        while self.current_bytes + size > CACHE_MAX_BYTES {
            match self.inner.pop_lru() {
                Some((_, evicted)) => {
                    self.current_bytes = self.current_bytes.saturating_sub(evicted.bytes.len());
                }
                None => break,
            }
        }
        // 条目数也到顶时 LruCache::put 会静默淘汰最旧条目、字节无从扣账——先手动弹出扣账，
        // 否则 current_bytes 单调虚高造成过度驱逐（账目漂移，方向安全但缓存容量缩水）
        if self.inner.len() >= self.inner.cap().get() {
            if let Some((_, evicted)) = self.inner.pop_lru() {
                self.current_bytes = self.current_bytes.saturating_sub(evicted.bytes.len());
            }
        }
        self.current_bytes += size;
        self.inner.put(key, img);
    }
}

static CACHE: OnceLock<Mutex<ByteLru>> = OnceLock::new();

fn cache() -> &'static Mutex<ByteLru> {
    CACHE.get_or_init(|| Mutex::new(ByteLru::new()))
}

/// Claude 会话数据 projects 根目录
fn projects_dir() -> PathBuf {
    crate::config::projects_dir()
}

/// 白名单校验：仅允许 [A-Za-z0-9._-]，拒绝空串。
/// 路径段安全校验（黑名单式）。拒绝：空段、`.`、`..`、路径分隔符（`/` `\`）、
/// 残留百分号（decode-once 设计——parse_request 只解一层 percent，段内残留 `%`
/// 意味着二次编码混淆企图，直接拒绝）、控制字符。
/// 其余放行，含非 ASCII——项目目录名（编码后的 cwd）可能含中文等多字节字符，
/// ASCII-only 白名单会让这类项目的历史图片整体 404（相对 base64 时代的功能回归）。
fn is_safe_segment(s: &str) -> bool {
    if s.is_empty() || s == ".." || s == "." {
        return false;
    }
    !s.bytes()
        .any(|b| b == b'/' || b == b'\\' || b == b'%' || b < 0x20 || b == 0x7f)
}

/// 解析 ccimg URL 的结果。
struct ParsedRequest {
    jsonl_path: PathBuf,
    record_uuid: String,
    img_index: usize,
    /// 缓存 key 用的稳定字符串前缀（路径 + agent）
    cache_scope: String,
    /// ?full=1 → 原图直出（点击放大）；默认返回服务端缩略图。
    /// 缩略图是图形内存治理的核心：前端 <img> 显示 300×200 但 WebKit 按原始像素
    /// 解码持有 IOSurface 纹理，2K 截图一张 14MB——服务端降采样后只剩 ~1/10
    full: bool,
}

/// 从请求 URI 解析出定位信息，含全部安全校验。失败返回 None。
fn parse_request(req: &Request<Vec<u8>>) -> Option<ParsedRequest> {
    let uri = req.uri();
    // path 形如 `/{project_id}/{session_id}/{record_uuid}/{img_index}`
    // project_id 段被 URL-encode（目录名含 `-` 无需 encode，但含 `/`→`-` 的编码规则下
    // project_id 本身不含 `/`；前端 convertFileSrc 仍会对整段做 percent-encode 兜底）。
    let raw_path = uri.path();
    // 前端 convertFileSrc 对整段 path 做 encodeURIComponent，四段间的 `/` 被编成 `%2F`，
    // 故 webview 发来的 uri.path() 形如 `/{p}%2F{s}%2F{u}%2F{i}`（整段一个 path segment）。
    // 必须先对整条 path percent-decode 还原出真正的 `/`，再按 `/` 切分——否则 split('/')
    // 只得到 1 段，segments.len() != 4 直接 404。decode 后各段不再二次 decode（避免
    // `%252F` 被二次解出 `/` 造成穿越；下方 is_safe_segment 亦会拒绝残留的 `/`）。
    let decoded_path = percent_decode(raw_path);
    let segments: Vec<String> = decoded_path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    if segments.len() != 4 {
        return None;
    }
    let project_id = &segments[0];
    let session_id = &segments[1];
    let record_uuid = &segments[2];
    let img_index_str = &segments[3];

    // 白名单校验（防目录穿越）
    if !is_safe_segment(project_id) || !is_safe_segment(session_id) || !is_safe_segment(record_uuid)
    {
        return None;
    }
    let img_index: usize = img_index_str.parse().ok()?;

    // 可选 ?agent=<id> → 子会话路径；?full=1 → 原图直出
    let mut agent_id: Option<String> = None;
    let mut full = false;
    if let Some(q) = uri.query() {
        for pair in q.split('&') {
            let mut it = pair.splitn(2, '=');
            match (it.next(), it.next()) {
                (Some("agent"), Some(v)) => agent_id = Some(percent_decode(v)),
                (Some("full"), Some("1")) => full = true,
                _ => {}
            }
        }
    }

    let (jsonl_path, cache_scope) = match agent_id {
        Some(agent) => {
            if !is_safe_segment(&agent) {
                return None;
            }
            let path = projects_dir()
                .join(project_id)
                .join(session_id)
                .join("subagents")
                .join(format!("agent-{}.jsonl", agent));
            let scope = format!("{}/{}/agent-{}", project_id, session_id, agent);
            (path, scope)
        }
        None => {
            let path = projects_dir()
                .join(project_id)
                .join(format!("{}.jsonl", session_id));
            let scope = format!("{}/{}", project_id, session_id);
            (path, scope)
        }
    };

    Some(ParsedRequest {
        jsonl_path,
        record_uuid: record_uuid.clone(),
        img_index,
        cache_scope,
        full,
    })
}

/// 极简 percent-decode（仅解 %XX，标准库实现避免引入 percent-encoding crate）。
/// 非法转义（越界或非 hex）原样保留 `%`。
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            match (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push((h << 4) | l);
                    i += 3;
                    continue;
                }
                _ => {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ============================================================================
// image block 深度优先遍历 —— JSON 路径（与 parser::walk_blocks_assign 同一契约）
// ----------------------------------------------------------------------------
// 顺序：顶层 message.content 数组按序；遇到 tool_result 且 content 为数组时先递归
// 其内嵌 blocks 再继续外层。序号从 0 起。此处从 raw JSON 提取第 target 个 image 的
// source.data（base64）。必须与 parser 的 typed 注入产出完全一致的序号。
// ============================================================================

/// 在一条 record 的 JSON 里深度优先找第 `target` 个 image block，返回其 base64 与 media_type。
fn find_nth_image(record: &Value, target: usize) -> Option<(String, String)> {
    let content = record.get("message")?.get("content")?;
    let blocks = content.as_array()?;
    let mut counter = 0usize;
    walk_blocks_find(blocks, target, &mut counter)
}

/// 深度优先遍历 JSON blocks，命中第 target 个 image 时返回 (base64, media_type)。
fn walk_blocks_find(
    blocks: &[Value],
    target: usize,
    counter: &mut usize,
) -> Option<(String, String)> {
    for block in blocks {
        let btype = block.get("type").and_then(|t| t.as_str());
        match btype {
            Some("image") => {
                if *counter == target {
                    // 命中即终结、序号即消耗（counter 越过 target 作为「已命中」标记）。
                    // source/data 缺失的畸形块返回 None → 404——绝不能让后续图片
                    // 顶替该序号，否则与 typed 注入口径错位造成静默错图
                    *counter += 1;
                    let data = block
                        .get("source")
                        .and_then(|s| s.get("data"))
                        .and_then(|d| d.as_str())?
                        .to_string();
                    let media = block
                        .get("source")
                        .and_then(|s| s.get("media_type"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("application/octet-stream")
                        .to_string();
                    return Some((data, media));
                }
                *counter += 1;
            }
            Some("tool_result") => {
                // content 为数组（Blocks）时递归；为字符串则无内嵌 image
                if let Some(inner) = block.get("content").and_then(|c| c.as_array()) {
                    if let Some(found) = walk_blocks_find(inner, target, counter) {
                        return Some(found);
                    }
                    // 内层已命中 target（counter 越过）但取图失败（畸形块）：
                    // 终结整个搜索，向上传播 404
                    if *counter > target {
                        return None;
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// 逐行扫 JSONL 找到 uuid == record_uuid 的行并反序列化为 Value。
fn load_record(jsonl_path: &PathBuf, record_uuid: &str) -> Option<Value> {
    let file = std::fs::File::open(jsonl_path).ok()?;
    let reader = BufReader::with_capacity(64 * 1024, file);
    // 快速预筛：uuid 字符串必须出现在行内才做完整反序列化
    let needle = format!("\"{}\"", record_uuid);
    for line in reader.lines() {
        let line = match line {
            Ok(l) if !l.trim().is_empty() => l,
            _ => continue,
        };
        if !line.contains(&needle) {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(&line) {
            if value.get("uuid").and_then(|u| u.as_str()) == Some(record_uuid) {
                return Some(value);
            }
        }
    }
    None
}

/// 缩略图长边上限：缩略图 CSS 显示 max 300×200，@2x Retina 需 600 物理 px，取 1.3x 余量。
const THUMB_LONG_EDGE: u32 = 800;

/// 服务端缩略图。返回 None 表示应原图直出（保守回退，宁可不省内存也不出错图）：
/// - GIF：重编码会丢动画帧
/// - 带 EXIF APP1 段的 JPEG：image crate 不解析 EXIF orientation，重编码会丢方向信息
///   导致照片旋转 90°/180°（截图无 APP1，主要内存来源不受影响）
/// - 长边已 ≤ 阈值 / 解码或编码失败
fn make_thumbnail(media_type: &str, bytes: &[u8]) -> Option<CachedImage> {
    if media_type.contains("gif") {
        return None;
    }
    if media_type.contains("jpeg") && has_exif_app1(bytes) {
        return None;
    }
    let img = image::load_from_memory(bytes).ok()?;
    if img.width().max(img.height()) <= THUMB_LONG_EDGE {
        return None;
    }
    let thumb = img.thumbnail(THUMB_LONG_EDGE, THUMB_LONG_EDGE);
    let mut buf = Vec::new();
    if thumb.color().has_alpha() {
        thumb
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .ok()?;
        Some(CachedImage {
            media_type: "image/png".to_string(),
            bytes: buf,
        })
    } else {
        let mut cursor = std::io::Cursor::new(&mut buf);
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 82);
        enc.encode_image(&thumb.to_rgb8()).ok()?;
        drop(enc);
        Some(CachedImage {
            media_type: "image/jpeg".to_string(),
            bytes: buf,
        })
    }
}

/// JPEG 是否含 EXIF APP1 段（marker 0xFFE1 + "Exif\0\0"）。只扫头部 marker 链。
fn has_exif_app1(bytes: &[u8]) -> bool {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return false;
    }
    let mut i = 2;
    // JPEG marker 链：APP 段都在图像数据（SOS 0xFFDA）之前
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            return false;
        }
        let marker = bytes[i + 1];
        if marker == 0xDA {
            return false;
        }
        let seg_len = ((bytes[i + 2] as usize) << 8) | bytes[i + 3] as usize;
        if marker == 0xE1 {
            return bytes
                .get(i + 4..i + 10)
                .map(|m| m == b"Exif\0\0")
                .unwrap_or(false);
        }
        i += 2 + seg_len;
    }
    false
}

/// 核心解析（同步、阻塞 I/O）：返回解码后的图片或 None（404）。
/// 缓存条目按「URL 语义」存：thumb/full 各自独立 key，一个 key 对应该 URL 的最终响应字节。
fn resolve_image(parsed: &ParsedRequest) -> Option<CachedImage> {
    let variant = if parsed.full { "full" } else { "thumb" };
    let cache_key = format!(
        "{}|{}|{}|{}",
        parsed.cache_scope, parsed.record_uuid, parsed.img_index, variant
    );
    // 缓存命中
    if let Ok(mut c) = cache().lock() {
        if let Some(hit) = c.get(&cache_key) {
            return Some(hit);
        }
    }
    // miss：读 JSONL → 找 record → 取第 N 个 image → decode
    let record = load_record(&parsed.jsonl_path, &parsed.record_uuid)?;
    let (b64, media_type) = find_nth_image(&record, parsed.img_index)?;
    let bytes = STANDARD.decode(b64.as_bytes()).ok()?;
    let original = CachedImage { media_type, bytes };
    // thumb 变体：降采样成功用缩略图，回退场景（GIF/EXIF/小图/解码失败）原图顶替
    let img = if parsed.full {
        original
    } else {
        make_thumbnail(&original.media_type, &original.bytes).unwrap_or(original)
    };
    if let Ok(mut c) = cache().lock() {
        c.put(cache_key, img.clone());
    }
    Some(img)
}

/// 中立 Engine asset 路由对 Claude 图片的兼容入口。
/// 仅返回解码后的原始字节，不暴露 projects 目录拼接给 Core。
pub fn resolve_engine_image(
    project_id: &str,
    session_id: &str,
    record_uuid: &str,
    img_index: usize,
) -> Option<(String, Vec<u8>)> {
    if !is_safe_segment(project_id) || !is_safe_segment(session_id) || !is_safe_segment(record_uuid)
    {
        return None;
    }
    let jsonl_path = projects_dir()
        .join(project_id)
        .join(format!("{session_id}.jsonl"));
    let parsed = ParsedRequest {
        cache_scope: jsonl_path.to_string_lossy().to_string(),
        jsonl_path,
        record_uuid: record_uuid.to_string(),
        img_index,
        full: true,
    };
    resolve_image(&parsed).map(|image| (image.media_type, image.bytes))
}

/// 200 响应（含 Content-Type 与长缓存头）
fn ok_response(img: &CachedImage) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", &img.media_type)
        // 历史区图片内容不变（uuid+img_index 唯一确定），可长缓存
        .header("Cache-Control", "public, max-age=31536000, immutable")
        .header("Access-Control-Allow-Origin", "*")
        .body(img.bytes.clone())
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

/// 404 响应
fn not_found() -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("Content-Type", "text/plain")
        .header("Access-Control-Allow-Origin", "*")
        .body(b"image not found".to_vec())
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

/// 协议 handler 入口。读文件走 tauri 异步线程池（spawn_blocking），不阻塞主线程。
pub fn handle_request(req: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let Some(parsed) = parse_request(&req) else {
        responder.respond(not_found());
        return;
    };
    tauri::async_runtime::spawn_blocking(move || {
        let response = match resolve_image(&parsed) {
            Some(img) => ok_response(&img),
            None => not_found(),
        };
        responder.respond(response);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 构造一条含顶层图 + tool_result 嵌套图的 user 记录 JSONL 行。
    /// 顺序：text, image(顶层#0), tool_result{ text, image(嵌套#1) }, image(顶层#2)
    fn sample_line(uuid: &str) -> String {
        // 三张不同的 1 字节 base64：'A'*... 用可 decode 的短串
        // "AQ==" → [0x01], "Ag==" → [0x02], "Aw==" → [0x03]
        format!(
            r#"{{"type":"user","uuid":"{uuid}","message":{{"role":"user","content":[{{"type":"text","text":"hi"}},{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"AQ=="}}}},{{"type":"tool_result","tool_use_id":"t1","content":[{{"type":"text","text":"r"}},{{"type":"image","source":{{"type":"base64","media_type":"image/jpeg","data":"Ag=="}}}}]}},{{"type":"image","source":{{"type":"base64","media_type":"image/gif","data":"Aw=="}}}}]}}}}"#
        )
    }

    fn write_jsonl(name: &str, lines: &[String]) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        for l in lines {
            writeln!(f, "{}", l).unwrap();
        }
        drop(f);
        path
    }

    fn encode_png(img: image::DynamicImage) -> Vec<u8> {
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    /// 大不透明图 → 缩到长边 800、输出 JPEG
    #[test]
    fn thumbnail_downscales_opaque_to_jpeg() {
        let src = image::DynamicImage::ImageRgb8(image::RgbImage::new(1600, 1200));
        let out = make_thumbnail("image/png", &encode_png(src)).expect("应生成缩略图");
        assert_eq!(out.media_type, "image/jpeg");
        let decoded = image::load_from_memory(&out.bytes).unwrap();
        assert!(decoded.width().max(decoded.height()) <= THUMB_LONG_EDGE);
        // 等比：1600×1200 → 800×600
        assert_eq!((decoded.width(), decoded.height()), (800, 600));
    }

    /// 带 alpha 的图 → 保 PNG（JPEG 会丢透明）
    #[test]
    fn thumbnail_preserves_alpha_as_png() {
        let src = image::DynamicImage::ImageRgba8(image::RgbaImage::new(2000, 1000));
        let out = make_thumbnail("image/png", &encode_png(src)).expect("应生成缩略图");
        assert_eq!(out.media_type, "image/png");
    }

    /// 小图 / GIF / 坏字节 → None（原图直出）
    #[test]
    fn thumbnail_falls_back_to_original() {
        let small = image::DynamicImage::ImageRgb8(image::RgbImage::new(400, 300));
        assert!(make_thumbnail("image/png", &encode_png(small)).is_none());
        assert!(make_thumbnail("image/gif", &[0u8; 10]).is_none());
        assert!(make_thumbnail("image/png", &[0u8; 10]).is_none());
    }

    /// EXIF APP1 探测：APP1+Exif 命中；APP0(JFIF)/非 JPEG 不命中
    #[test]
    fn exif_app1_detection() {
        let mut with_exif = vec![0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x08];
        with_exif.extend_from_slice(b"Exif\0\0");
        assert!(has_exif_app1(&with_exif));

        let jfif = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x4A, 0x46];
        assert!(!has_exif_app1(&jfif));
        assert!(!has_exif_app1(b"not a jpeg"));
    }

    /// F1 交叉验证（契约核心）：同一批 blocks，typed 注入（parser::walk_blocks_assign）
    /// 与 raw 提取（walk_blocks_find）的 image 序号必须完全一致——含畸形块。
    /// 计数口径 = 「type == "image" 即计数」：缺 media_type / 缺 source 的畸形块也占号
    /// （typed 侧靠 ImageSource 全字段 default 保证不落 Unknown）。
    #[test]
    fn traversal_order_matches_typed_injection() {
        use crate::models::{ContentBlock, ToolResultContent};

        fn collect_image_indices(blocks: &[ContentBlock]) -> Vec<u32> {
            let mut out = vec![];
            for b in blocks {
                match b {
                    ContentBlock::Image { source } => out.push(source.img_index),
                    ContentBlock::ToolResult {
                        content: ToolResultContent::Blocks(inner),
                        ..
                    } => out.extend(collect_image_indices(inner)),
                    _ => {}
                }
            }
            out
        }

        // 顶层图#0 → tool_result{ 正常图#1, 缺media_type图#2, 缺source图#3 } → 顶层图#4
        let blocks_json = r#"[
            {"type":"image","source":{"type":"base64","media_type":"image/png","data":"AA=="}},
            {"type":"tool_result","tool_use_id":"t1","content":[
                {"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"AQ=="}},
                {"type":"image","source":{"type":"base64","data":"Ag=="}},
                {"type":"image"}
            ]},
            {"type":"image","source":{"type":"base64","media_type":"image/webp","data":"Aw=="}}
        ]"#;

        // typed 侧：反序列化 + 注入，序号必须深度优先连续（畸形块也计数）
        let mut typed: Vec<ContentBlock> = serde_json::from_str(blocks_json).unwrap();
        let mut counter = 0u32;
        crate::parser::walk_blocks_assign(&mut typed, &mut counter);
        assert_eq!(counter, 5, "typed 侧应数出 5 张图");
        assert_eq!(collect_image_indices(&typed), vec![0, 1, 2, 3, 4]);

        // raw 侧：逐序号提取，data 与 JSON 期望一一对应（None = 缺 source 的畸形块 404）
        let raw: Vec<serde_json::Value> = serde_json::from_str(blocks_json).unwrap();
        let expect = [Some("AA=="), Some("AQ=="), Some("Ag=="), None, Some("Aw==")];
        for (i, want) in expect.iter().enumerate() {
            let mut c = 0usize;
            let got = walk_blocks_find(&raw, i, &mut c);
            assert_eq!(
                got.as_ref().map(|(d, _)| d.as_str()),
                *want,
                "raw 提取 target={i} 与期望不符"
            );
        }
        let mut c = 0usize;
        assert!(
            walk_blocks_find(&raw, 5, &mut c).is_none(),
            "两侧图片总数必须一致"
        );
    }

    /// 白名单放宽后的回归：非 ASCII 段（中文项目目录）放行，穿越与残留 % 仍拒
    #[test]
    fn safe_segment_allows_non_ascii_rejects_traversal() {
        assert!(is_safe_segment("-Users-xt-项目-测试"));
        assert!(is_safe_segment("normal-id_1.2"));
        assert!(!is_safe_segment(".."));
        assert!(!is_safe_segment("a/b"));
        assert!(!is_safe_segment("a\\b"));
        assert!(!is_safe_segment("a%2Fb"));
        assert!(!is_safe_segment(""));
    }

    #[test]
    fn top_level_image_index_0() {
        let path = write_jsonl("ccimg-test-top.jsonl", &[sample_line("uuid-top")]);
        let record = load_record(&path, "uuid-top").unwrap();
        let (b64, media) = find_nth_image(&record, 0).unwrap();
        assert_eq!(b64, "AQ==");
        assert_eq!(media, "image/png");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn nested_tool_result_image_index_1() {
        let path = write_jsonl("ccimg-test-nested.jsonl", &[sample_line("uuid-nest")]);
        let record = load_record(&path, "uuid-nest").unwrap();
        // 嵌套图是深度优先第 1 个（顶层图#0 之后、末尾顶层图#2 之前）
        let (b64, media) = find_nth_image(&record, 1).unwrap();
        assert_eq!(b64, "Ag==");
        assert_eq!(media, "image/jpeg");
        // 末尾顶层图应为 #2
        let (b64_2, media_2) = find_nth_image(&record, 2).unwrap();
        assert_eq!(b64_2, "Aw==");
        assert_eq!(media_2, "image/gif");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn uuid_not_found_returns_none() {
        let path = write_jsonl("ccimg-test-nouuid.jsonl", &[sample_line("uuid-x")]);
        assert!(load_record(&path, "does-not-exist").is_none());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn img_index_out_of_range_returns_none() {
        let path = write_jsonl("ccimg-test-oob.jsonl", &[sample_line("uuid-oob")]);
        let record = load_record(&path, "uuid-oob").unwrap();
        // 只有 3 张图（0,1,2），索引 3 越界
        assert!(find_nth_image(&record, 3).is_none());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn path_traversal_rejected() {
        // 白名单拒绝穿越段
        assert!(!is_safe_segment(".."));
        assert!(!is_safe_segment("."));
        assert!(!is_safe_segment("a/b"));
        assert!(!is_safe_segment("a\\b"));
        assert!(!is_safe_segment("..%2f.."));
        assert!(!is_safe_segment(""));
        // 合法段放行
        assert!(is_safe_segment("-Users-xt-foo"));
        assert!(is_safe_segment("a1b2-c3d4.jsonl"));
        assert!(is_safe_segment("agent-123"));
    }

    /// 构造与前端 convertFileSrc(encodeURIComponent(path), 'ccimg') 完全一致的请求。
    /// 关键：四段间的 `/` 被 encodeURIComponent 编成 `%2F`，整条 path 是单个 segment。
    fn ccimg_request(
        project: &str,
        session: &str,
        uuid: &str,
        idx: u32,
        agent: Option<&str>,
    ) -> Request<Vec<u8>> {
        let inner = format!("{}/{}/{}/{}", project, session, uuid, idx).replace('/', "%2F");
        let url = match agent {
            Some(a) => format!("ccimg://localhost/{}?agent={}", inner, a),
            None => format!("ccimg://localhost/{}", inner),
        };
        Request::builder().uri(url).body(Vec::new()).unwrap()
    }

    /// 契约回归：前端 convertFileSrc 会对整段 path 做 encodeURIComponent，`/`→`%2F`。
    /// parse_request 必须先整体 percent-decode 再切分，才能还原 4 段——否则历史区图片全 404。
    #[test]
    fn parse_request_decodes_encoded_slashes() {
        let req = ccimg_request("-Users-xt-proj", "sess-1", "uuid-abc", 2, None);
        let parsed = parse_request(&req).expect("应解析出 4 段而非 404");
        assert_eq!(parsed.record_uuid, "uuid-abc");
        assert_eq!(parsed.img_index, 2);
        assert_eq!(parsed.cache_scope, "-Users-xt-proj/sess-1");
        assert!(parsed
            .jsonl_path
            .to_string_lossy()
            .ends_with("-Users-xt-proj/sess-1.jsonl"));
    }

    /// 子 Agent 路径：`?agent=` query 定位 subagents/agent-<id>.jsonl。
    #[test]
    fn parse_request_subagent_query() {
        let req = ccimg_request("-Users-xt-proj", "sess-1", "uuid-x", 0, Some("ag-9"));
        let parsed = parse_request(&req).expect("子 agent 应解析成功");
        assert_eq!(parsed.cache_scope, "-Users-xt-proj/sess-1/agent-ag-9");
        assert!(parsed
            .jsonl_path
            .to_string_lossy()
            .ends_with("-Users-xt-proj/sess-1/subagents/agent-ag-9.jsonl"));
    }

    /// 穿越防护仍在：即便 `%2F` 解出真实 `/`，多出的段数或非法段都被拒。
    #[test]
    fn parse_request_rejects_traversal_after_decode() {
        // 多注入一段(../ 编码)使 decode 后段数≠4 或含非法段
        let inner = "-Users-xt-proj/../etc/sess/uuid/0".replace('/', "%2F");
        let url = format!("ccimg://localhost/{}", inner);
        let req = Request::builder().uri(url).body(Vec::new()).unwrap();
        assert!(parse_request(&req).is_none());
    }
}
