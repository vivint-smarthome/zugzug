use fs_extra::{copy_items, dir::CopyOptions};
use regex::Regex;
use ruplacer::{query::Query, DirectoryPatcher};

use std::env;
use std::fs::{copy, create_dir, remove_dir_all};
use std::path::PathBuf;
use std::process::Command;

fn main() {
  println!("cargo:rerun-if-changed=build.rs");

  // TODO: cross compile
  // TODO: use variables for paths, etc.
  let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());

  remove_dir_all(&out_path).unwrap();
  create_dir(&out_path).unwrap();

  let upstream_build_dir = out_path.join("c-core");

  let upstream_build_dir_posix = upstream_build_dir.join("posix");

  copy_items(
    &vec!["vendor/c-core"],
    &upstream_build_dir.display().to_string(),
    &CopyOptions {
      copy_inside: true,
      overwrite: true,
      ..CopyOptions::new()
    },
  )
  .unwrap();

  DirectoryPatcher::new(upstream_build_dir_posix.join("posix.mk"), Default::default())
    .patch(&Query::Regex(
      Regex::new(r"CFLAGS =.*").unwrap(),
      "${0}\nCFLAGS += -fPIC".to_owned(),
    ))
    .unwrap();

  DirectoryPatcher::new(upstream_build_dir.join("lib/"), Default::default())
    .patch(&Query::Regex(
      Regex::new(r"MD5_(Update|Init|Final)\b").unwrap(),
      "${0}_vendor".to_owned(),
    ))
    .unwrap();

  #[cfg(feature = "static")]
  {
    Command::new("make")
      .current_dir(upstream_build_dir_posix.clone())
      .args(&["pubnub_sync.a", "pubnub_callback.a", "-f", "posix.mk"])
      .status()
      .unwrap();
    println!("cargo:rustc-link-search={}", upstream_build_dir_posix.display());
  }

  #[cfg(feature = "callback")]
  {
    #[cfg(feature = "static")]
    {
      copy(
        upstream_build_dir_posix.join("pubnub_callback.a"),
        upstream_build_dir_posix.join("libpubnub_callback.a"),
      )
      .unwrap();
      println!("cargo:rustc-link-lib=static=pubnub_callback");
    }

    let callback_bindings = bindgen::Builder::default()
    .header(format!("{}/pubnub_callback.h", upstream_build_dir_posix.display()))
    .clang_arg(format!("-I{}", upstream_build_dir.display()))
    .clang_arg(format!("-I{}", upstream_build_dir_posix.display()))
    .clang_arg("-DPUBNUB_CALLBACK_API=1")
    .clang_arg("-DPUBNUB_THREADSAFE=1") // Makes contexts thread-safe, justifying our making them Send and Sync.
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
      copy(
        upstream_build_dir_posix.join("pubnub_sync.a"),
        upstream_build_dir_posix.join("libpubnub_sync.a"),
      )
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

  let dns_bindings = bindgen::Builder::default()
  .header(format!("{}/core/pubnub_dns_servers.h", upstream_build_dir.display()))
  .clang_arg(format!("-I{}", upstream_build_dir.display()))
  .clang_arg(format!("-I{}", upstream_build_dir_posix.display()))
  .clang_arg("-DPUBNUB_CALLBACK_API=1")
  .clang_arg("-DPUBNUB_SET_DNS_SERVERS=1")
  .clang_arg("-DPUBNUB_THREADSAFE=1") // Makes contexts thread-safe, justifying our making them Send and Sync.
  .blacklist_function("strtold") // u128 is not ffi-safe
  .generate()
  .expect("Unable to generate dns bindings");

  dns_bindings
    .write_to_file(out_path.join("dns.rs"))
    .expect("Couldn't write bindings");
}
