fn main() {
  // ProMotion 高刷解锁：ObjC swizzle 必须在 WKWebView 创建前就位，
  // 见 src/native/high_refresh.m 顶部注释
  #[cfg(target_os = "macos")]
  {
    cc::Build::new()
      .file("src/native/high_refresh.m")
      .flag("-fobjc-arc")
      .compile("monet_high_refresh");
    println!("cargo:rerun-if-changed=src/native/high_refresh.m");
    println!("cargo:rustc-link-lib=framework=WebKit");

    // 工作台全景导出：由真实 WKWebView 扩宽布局并生成原生快照。
    cc::Build::new()
      .file("src/native/webview_snapshot.m")
      .flag("-fobjc-arc")
      .compile("monet_webview_snapshot");
    println!("cargo:rerun-if-changed=src/native/webview_snapshot.m");
    println!("cargo:rustc-link-lib=framework=AppKit");

    // TCC 权限静默检测（设置页权限体检），主 App 与 routine-runner 共用
    cc::Build::new()
      .file("src/native/tcc_check.c")
      .compile("monet_tcc_check");
    println!("cargo:rerun-if-changed=src/native/tcc_check.c");
    println!("cargo:rustc-link-lib=framework=CoreServices");
    println!("cargo:rustc-link-lib=framework=ApplicationServices");

    // 本地网络权限探测：必须走 Network.framework（受本地网络隐私管辖的路径），
    // 用 blocks + GCD 把异步连接包成同步调用，故为 .m
    cc::Build::new()
      .file("src/native/local_network.m")
      .flag("-fobjc-arc")
      .compile("monet_local_network");
    println!("cargo:rerun-if-changed=src/native/local_network.m");
    println!("cargo:rustc-link-lib=framework=Network");

    // macOS 13+ 后台项目注册：SMAppService 取代手写 LaunchAgent / LoginItem。
    cc::Build::new()
      .file("src/native/service_management.m")
      .flag("-fobjc-arc")
      .compile("monet_service_management");
    println!("cargo:rerun-if-changed=src/native/service_management.m");
    println!("cargo:rustc-link-lib=framework=ServiceManagement");

    // App Group 容器必须由 FileManager 解析，不能手拼 ~/Library/Group Containers。
    cc::Build::new()
      .file("src/native/app_group.m")
      .flag("-fobjc-arc")
      .compile("monet_app_group");
    println!("cargo:rerun-if-changed=src/native/app_group.m");
    println!("cargo:rustc-link-lib=framework=Foundation");

    // rustc-link-lib 只随 lib target 传播；runner bin 不依赖 app lib
    //（避免链入 tauri），需要按 bin 显式补链接参数
    let out_dir = std::env::var("OUT_DIR").unwrap();
    for lib in ["libmonet_tcc_check.a", "libmonet_local_network.a"] {
      println!(
        "cargo:rustc-link-arg-bin=monet-routine-runner={}/{}",
        out_dir, lib
      );
    }
    for arg in [
      "-framework", "CoreServices",
      "-framework", "ApplicationServices",
      "-framework", "Network",
    ] {
      println!("cargo:rustc-link-arg-bin=monet-routine-runner={}", arg);
    }

    // widget-updater 是独立 bin，不链接 app lib，需要显式带上 App Group 桥接库。
    println!(
      "cargo:rustc-link-arg-bin=widget-updater={}/libmonet_app_group.a",
      out_dir
    );
    println!("cargo:rustc-link-arg-bin=widget-updater=-framework");
    println!("cargo:rustc-link-arg-bin=widget-updater=Foundation");

    // 裸二进制嵌入 Info.plist（__TEXT,__info_plist 段）：TCC 要求发送
    // Apple Events / 本地网络请求的进程带用途说明，缺失时授权请求可能被
    // 系统静默丢弃。launchd 直启的 runner 没有外层 bundle，只能链接期嵌入。
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!(
      "cargo:rustc-link-arg-bin=monet-routine-runner=-Wl,-sectcreate,__TEXT,__info_plist,{}/runner-info.plist",
      manifest_dir
    );
    println!("cargo:rerun-if-changed=runner-info.plist");
  }
  tauri_build::build()
}
