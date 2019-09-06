use futures::stream::Stream;
use futures::sync::mpsc::{Receiver, Sender};
use futures::task::Task;
use futures::{Async, Future};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::ffi::CString;

use zugzug_sys::callback::*;

// TODO: see if we can pull out the channel stuff, etc. and subscribe to multiple channels; we probably should just return a `Subscription` which implements `Stream`.  This would also allow us to have a single client that produces subscription `Stream`s and publish `Future`s.
pub struct ClientConfig {
  pub auth_key: String,
  pub publish_key: String,
  pub subscribe_key: String,
  pub channel: String,
  pub group: String,
  pub client_uuid: String,
}

struct SubscribeUserData<T> {
  channel: CString,
  tx: Sender<Result<T, ClientError>>,
}

#[allow(unused)] // Because we want to keep the CStrings from being dropped while we have pointers to them.
pub struct SubscribeClient<T> {
  ctx: *mut pubnub_t,
  // We hold on to this so that we can free the memory later.
  user_data: *mut SubscribeUserData<T>,
  // We pass refs of these to C land.  We keep them around here so they will not be freed until the `Client` is dropped.
  channel: CString,
  auth_key: CString,
  publish_key: CString,
  subscribe_key: CString,
  group: CString,
  client_uuid: CString,
}

unsafe extern "C" fn subscribe_callback<'a, T: Deserialize<'a>>(
  pb: *mut pubnub_t,
  trans: pubnub_trans,
  result: pubnub_res,
  user_data: *mut ::std::os::raw::c_void,
) {
  let ud: &mut SubscribeUserData<T> = &mut *(user_data as *mut SubscribeUserData<T>); // TODO: verify that this callback can only happen once at a time, or wrap in a mutex.
  if trans == pubnub_trans_PBTT_SUBSCRIBE {
    let res: Result<T, ClientError> = if result == pubnub_res_PNR_OK {
      let ptr = pubnub_get(pb);
      if !ptr.is_null() {
        let c = std::ffi::CStr::from_ptr(ptr);
        let s = c.to_str().unwrap(); // TODO: return error if that is needed.

        serde_json::from_str::<T>(s).map_err(|e| ClientError::ParseError(e))
      } else {
        Err(ClientError::NullPointerError)
      }
    } else {
      Err(ClientError::PubNub { code: result })
    };
    ud.tx
      .try_send(res)
      .map_err(|e| println!("subscribe callback unable to send {:?}", e))
      .ok();
  }

  // TODO: verify that we are happy with this here.  PubNub docs suggest that it is ok to do operations like this inside of a callback, but not recommended (as it can make debugging harder).  Our use case is simple (a loop), so maybe we're ok?
  pubnub_subscribe(pb, ud.channel.as_ptr(), std::ptr::null());
}

impl<'a, T: Send + Sync + Deserialize<'a>> SubscribeClient<T> {
  /// Starts a client, subscribed to the given channel, etc.
  pub fn start(config: ClientConfig, tx: Sender<Result<T, ClientError>>) -> Result<Self, Box<dyn Error>> {
    // TODO: client errors
    let auth_key = CString::new(config.auth_key)?;
    let publish_key = CString::new(config.publish_key)?;
    let subscribe_key = CString::new(config.subscribe_key)?;
    let channel = CString::new(config.channel)?;
    let group = CString::new(config.group)?;
    let client_uuid = CString::new(config.client_uuid)?;

    let user_data = Box::into_raw(Box::new(SubscribeUserData {
      tx,
      channel: channel.clone(),
    }));

    let ctx = unsafe {
      let ctx = pubnub_alloc();
      pubnub_init(ctx, publish_key.as_ptr(), subscribe_key.as_ptr());
      pubnub_set_uuid(ctx, client_uuid.as_ptr());
      pubnub_set_auth(ctx, auth_key.as_ptr());
      pubnub_register_callback(ctx, Some(subscribe_callback::<T>), user_data as *mut std::ffi::c_void);
      pubnub_subscribe(ctx, channel.as_ptr(), std::ptr::null());
      ctx
    };

    // TODO: verify that I'm not invalidating the above pointers at this point.
    Ok(Self {
      ctx,
      channel,
      auth_key,
      publish_key,
      subscribe_key,
      group,
      client_uuid,
      user_data,
    })
  }
}

impl<T> Drop for SubscribeClient<T> {
  fn drop(&mut self) {
    unsafe {
      pubnub_free(self.ctx);
      Box::from_raw(self.user_data); // This should be safe because we have cleared the mutable ref held by self.ctx
    };
  }
}

// TODO: have a single client.
pub struct PublishClient {
  // We pass refs of these to C land.  We keep them around here so they will not be freed until the `Client` is dropped.
  channel: CString,
  auth_key: CString,
  publish_key: CString,
  subscribe_key: CString,
  group: CString,
  client_uuid: CString,
}

impl PublishClient {
  /// Starts a client, subscribed to the given channel, etc.
  pub fn new(config: ClientConfig) -> Result<Self, Box<dyn Error>> {
    // TODO: client errors
    let auth_key = CString::new(config.auth_key)?;
    let publish_key = CString::new(config.publish_key)?;
    let subscribe_key = CString::new(config.subscribe_key)?;
    let channel = CString::new(config.channel)?;
    let group = CString::new(config.group)?;
    let client_uuid = CString::new(config.client_uuid)?;

    Ok(Self {
      channel,
      auth_key,
      publish_key,
      subscribe_key,
      group,
      client_uuid,
    })
  }

  pub fn publish<U: Serialize>(&mut self, msg: U) -> PublishFuture {
    PublishFuture::new(
      msg,
      self.channel.clone(),       // TODO: avoid the clone
      self.auth_key.clone(),      // TODO: avoid the clone
      self.publish_key.clone(),   // TODO: avoid the clone
      self.subscribe_key.clone(), // TODO: avoid the clone
      self.group.clone(),         // TODO: avoid the clone
      self.client_uuid.clone(),   // TODO: avoid the clone
    )
  }
}

struct PublishFutureUserData {
  task: Task,
  tx: Sender<Result<(), ClientError>>,
}

unsafe extern "C" fn publish_callback(
  _pb: *mut pubnub_t,
  trans: pubnub_trans,
  result: pubnub_res,
  user_data: *mut ::std::os::raw::c_void,
) {
  if trans == pubnub_trans_PBTT_PUBLISH {
    let res = if result == pubnub_res_PNR_OK {
      Ok(())
    } else {
      Err(ClientError::PubNub { code: result })
    };

    let ud: &mut PublishFutureUserData = &mut *(user_data as *mut PublishFutureUserData);
    ud.tx
      .try_send(res)
      .map_err(|e| println!("publish callback unable to send {:?}", e))
      .ok();
    ud.task.notify();
  }
}

#[allow(unused)]
pub struct PublishFuture {
  // This would be a oneshot, but we can't get ownership of the tx end in the callback (to send the message), so we use mpsc as if it were a oneshot.
  user_data: Option<*mut PublishFutureUserData>,
  rx: Option<Receiver<Result<(), ClientError>>>,
  ctx: *mut pubnub_t,
  channel: CString,
  auth_key: CString,
  publish_key: CString,
  subscribe_key: CString,
  group: CString,
  client_uuid: CString,
  msg: CString,
  started: bool,
}

impl PublishFuture {
  fn new<T: Serialize>(
    msg: T,
    channel: CString,
    auth_key: CString,
    publish_key: CString,
    subscribe_key: CString,
    group: CString,
    client_uuid: CString,
  ) -> Self {
    let msg = serde_json::to_string(&msg).unwrap();
    let msg_c = CString::new(msg).unwrap();
    let ctx = unsafe {
      let ctx = pubnub_alloc();
      pubnub_init(ctx, publish_key.as_ptr(), subscribe_key.as_ptr());
      pubnub_set_uuid(ctx, client_uuid.as_ptr());
      pubnub_set_auth(ctx, auth_key.as_ptr());
      ctx
    };

    Self {
      started: false,
      user_data: None,
      rx: None,
      ctx,
      channel,
      auth_key,
      publish_key,
      subscribe_key,
      group,
      client_uuid,
      msg: msg_c,
    }
  }
}

impl Drop for PublishFuture {
  fn drop(&mut self) {
    unsafe {
      pubnub_free(self.ctx);
      if let Some(ud) = self.user_data {
        Box::from_raw(ud); // This should be safe because we have cleared the mutable ref held by self.ctx
      }
    }
  }
}

impl Future for PublishFuture {
  type Item = ();
  type Error = ClientError;

  fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
    if !self.started {
      self.started = true;
      let (tx, rx) = futures::sync::mpsc::channel::<Result<(), ClientError>>(0);
      self.rx = Some(rx);
      let user_data = Box::into_raw(Box::new(PublishFutureUserData {
        tx,
        task: futures::task::current(),
      }));
      unsafe {
        pubnub_register_callback(self.ctx, Some(publish_callback), user_data as *mut std::ffi::c_void);
        pubnub_publish(self.ctx, self.channel.as_ptr(), self.msg.as_ptr());
      }
      self.user_data = Some(user_data);
      Ok(Async::NotReady)
    } else {
      if let Some(ref mut rx) = self.rx {
        match rx.poll() {
          Ok(Async::Ready(Some(Ok(())))) => Ok(Async::Ready(())),
          Ok(Async::Ready(Some(Err(e)))) => Err(e),
          Ok(Async::Ready(None)) => Ok(Async::NotReady),
          Ok(Async::NotReady) => Ok(Async::NotReady),
          Err(()) => Err(ClientError::PollError),
        }
      } else {
        panic!("rx ought to have been defined here");
      }
    }
  }
}

#[derive(Debug)]
pub enum ClientError {
  NullPointerError,
  ParseError(serde_json::Error),
  PollError,
  PubNub { code: pubnub_res },
}

impl std::fmt::Display for ClientError {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    match self {
      ClientError::NullPointerError => write!(f, "PubNub client null pointer error"),
      ClientError::ParseError(e) => e.fmt(f),
      ClientError::PollError => write!(f, "PubNub client poll error"),
      ClientError::PubNub { code } => write!(f, "PubNub client error with code {}", code), // TODO: it would be nice to do these codes as an enum, but bindgen does not recommend directly building enums, as we do not own the c code.
    }
  }
}

impl std::error::Error for ClientError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    match self {
      ClientError::NullPointerError => None,
      ClientError::ParseError(e) => e.source(),
      ClientError::PollError => None,
      ClientError::PubNub { code: _ } => None,
    }
  }
}
