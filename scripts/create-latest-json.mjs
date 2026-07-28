#!/usr/bin/env node
// 生成 Tauri updater 的版本清单 latest.json。
// 用法: node create-latest-json.mjs <version> <tarball-path> <out-path> [win-exe-path]
// win-exe 可选(CI 发版链路传入,本地 macOS 打包不传):NSIS 安装包本身即 Windows
// updater 工件,同目录须有 .exe.sig。
// 上传到 GitHub Release 后,应用内 updater 经 tauri.conf plugins.updater.endpoints
// 的 /releases/latest/download/latest.json 读取(draft release 不算 latest,
// 手动 publish 后才对用户生效)。
import { readFileSync, writeFileSync } from 'node:fs'
import { basename } from 'node:path'

const [version, tarball, outPath, winExe] = process.argv.slice(2)
if (!version || !tarball || !outPath) {
  console.error('usage: create-latest-json.mjs <version> <tarball> <out> [win-exe]')
  process.exit(1)
}

// 默认按正式发版的 v<version> tag 取资产；nightly 走固定的滚动 tag，
// 经 RELEASE_TAG 覆盖（版本号里已含日期，tag 无需再区分）
const tag = process.env.RELEASE_TAG || `v${version}`
const releaseUrl = (file) =>
  `https://github.com/zenolab124/monet/releases/download/${tag}/${basename(file)}`

// .sig 内容整体作为 signature 字段(tauri signer sign 的产物,base64 文本)
const signature = readFileSync(`${tarball}.sig`, 'utf8').trim()

const manifest = {
  version,
  pub_date: new Date().toISOString(),
  platforms: {
    'darwin-aarch64': {
      signature,
      url: releaseUrl(tarball),
    },
  },
}

if (winExe) {
  manifest.platforms['windows-x86_64'] = {
    signature: readFileSync(`${winExe}.sig`, 'utf8').trim(),
    url: releaseUrl(winExe),
  }
}

writeFileSync(outPath, `${JSON.stringify(manifest, null, 2)}\n`)
console.log(`latest.json → ${outPath}`)
