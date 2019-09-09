#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(unused)]
#[allow(clippy::all)]
#[cfg(feature = "callback")]
pub mod callback {
  include!(concat!(env!("OUT_DIR"), "/callback.rs"));
}

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(unused)]
#[allow(clippy::all)]
#[cfg(feature = "sync")]
pub mod sync {
  include!(concat!(env!("OUT_DIR"), "/sync.rs"));
}

#[cfg(test)]
#[cfg(feature = "sync")]
mod sync_test {
  #[test]
  fn smoke() {
    unsafe {
      let ctx = super::sync::pubnub_alloc();
      super::sync::pubnub_free(ctx);
    }
  }
}

#[cfg(test)]
#[cfg(feature = "callback")]
mod callback_test {
  #[test]
  fn smoke() {
    unsafe {
      let ctx = super::callback::pubnub_alloc();
      super::callback::pubnub_free(ctx);
    }
  }
}
