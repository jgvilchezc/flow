fn main() {
    // With MACOSX_DEPLOYMENT_TARGET below the host SDK, ggml's
    // __builtin_available checks become runtime calls to
    // ___isPlatformVersionAtLeast, which lives in clang's compiler-rt
    // builtins. rustc links with -nodefaultlibs, so that library must be
    // added explicitly or release linking fails.
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("clang")
            .arg("--print-resource-dir")
            .output()
        {
            let resource_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !resource_dir.is_empty() {
                println!("cargo:rustc-link-search={resource_dir}/lib/darwin");
                println!("cargo:rustc-link-lib=clang_rt.osx");
            }
        }
    }

    tauri_build::build()
}
