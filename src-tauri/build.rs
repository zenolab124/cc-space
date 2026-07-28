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
  }
  tauri_build::build()
}
