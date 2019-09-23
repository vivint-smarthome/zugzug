use regex::Regex;
use ruplacer::{query::Query, DirectoryPatcher};

use std::env;
use std::fs::{create_dir, remove_dir_all};
use std::path::PathBuf;
use std::process::Command;

fn main() {
  println!("cargo:rerun-if-changed=build.rs");

  // TODO: cross compile
  // TODO: use variables for paths, etc.
  // TODO: make this stuff more portable (e.g. no `Command`)
  let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());

  remove_dir_all(&out_path).unwrap();
  create_dir(&out_path).unwrap();

  let upstream_build_dir = out_path.join("c-core");

  let upstream_build_dir_posix = upstream_build_dir.join("posix");

  Command::new("cp")
    .args(&["-r", "vendor/c-core", &upstream_build_dir.display().to_string()])
    .status()
    .unwrap();

  #[cfg(feature = "static")]
  {
    Command::new("make")
      .current_dir(upstream_build_dir.clone())
      .args(&["-f", "posix.mk"])
      .status()
      .unwrap();
    println!("cargo:rustc-link-search={}", upstream_build_dir_posix.display());
  }

  #[cfg(feature = "callback")]
  {
    #[cfg(feature = "static")]
    {
      Command::new("cp")
        .args(&[
          &format!("{}", upstream_build_dir_posix.join("pubnub_callback.a").display()),
          &format!("{}", upstream_build_dir_posix.join("libpubnub_callback.a").display()),
        ])
        .status()
        .unwrap();
      println!("cargo:rustc-link-lib=static=pubnub_callback");
    }

    let callback_bindings = bindgen::Builder::default()
    .header(format!("{}/pubnub_callback.h", upstream_build_dir_posix.display()))
    .clang_arg(format!("-I{}", upstream_build_dir.display()))
    .clang_arg(format!("-I{}", upstream_build_dir_posix.display()))
    .clang_arg("-DPUBNUB_CALLBACK_API=1")
    .blacklist_function("strtold") // u128 is not ffi-safe
    .generate()
    .expect("Unable to generate callback bindings");

    callback_bindings
      .write_to_file(out_path.join("callback.rs"))
      .expect("Couldn't write bindings");

    #[cfg(feature = "dynamic")]
    println!("cargo:rustc-link-lib=pubnub_callback");
  }

  #[cfg(feature = "sync")]
  {
    #[cfg(feature = "static")]
    {
      Command::new("cp")
        .args(&[
          &format!("{}", upstream_build_dir_posix.join("pubnub_sync.a").display()),
          &format!("{}", upstream_build_dir_posix.join("libpubnub_sync.a").display()),
        ])
        .status()
        .unwrap();
      println!("cargo:rustc-link-lib=static=pubnub_sync");
    }

    let sync_bindings = bindgen::Builder::default()
    .header(format!("{}/pubnub_sync.h", upstream_build_dir_posix.display()))
    .clang_arg(format!("-I{}", upstream_build_dir.display()))
    .clang_arg(format!("-I{}", upstream_build_dir_posix.display()))
    .clang_arg("-DPUBNUB_CALLBACK_API=0")
    .blacklist_function("strtold") // u128 is not ffi-safe
    .generate()
    .expect("Unable to generate sync bindings");

    sync_bindings
      .write_to_file(out_path.join("sync.rs"))
      .expect("Couldn't write bindings");

    #[cfg(feature = "dynamic")]
    println!("cargo:rustc-link-lib=pubnub_sync");
  }
}
