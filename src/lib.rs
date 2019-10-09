use futures::stream::Stream;
use futures::sync::mpsc::{Receiver, Sender};
use futures::task::Task;
use futures::{Async, Future};
use serde::{Deserialize, Serialize};
use std::ffi::CString;

use zugzug_sys::callback::*;

#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Debug, Hash)]
pub struct ClientConfig {
  pub auth_key: String,
  pub publish_key: String,
  pub subscribe_key: String,
  pub client_uuid: String,
}

struct ChannelConfig {
  auth_key: CString,
  publish_key: CString,
  subscribe_key: CString,
  client_uuid: CString,
  channel: CString,
  group: CString,
}

struct SubscribeUserData<T> {
  channel: CString,
  tx: Sender<Result<T, ClientError>>,
}

#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Debug, Hash)]
pub struct Client {
  auth_key: CString,
  publish_key: CString,
  subscribe_key: CString,
  client_uuid: CString,
}

impl Client {
  pub fn new(config: ClientConfig) -> Self {
    let ClientConfig {
      auth_key,
      publish_key,
      subscribe_key,
      client_uuid,
    } = config;

    let auth_key = CString::new(auth_key).expect("UTF-8 doesn't include nul");
    let publish_key = CString::new(publish_key).expect("UTF-8 doesn't include nul");
    let subscribe_key = CString::new(subscribe_key).expect("UTF-8 doesn't include nul");
    let client_uuid = CString::new(client_uuid).expect("UTF-8 doesn't include nul");
    Self {
      auth_key,
      publish_key,
      subscribe_key,
      client_uuid,
    }
  }

  pub fn subscribe<'a, T: Send + Sync + Deserialize<'a>>(&self, channel: &str, group: &str) -> Subscription<T> {
    let channel_c = CString::new(channel).expect("UTF-8 doesn't include nul");
    let group_c = CString::new(group).expect("UTF-8 doesn't include nul");

    let config = ChannelConfig {
      auth_key: self.auth_key.clone(),
      publish_key: self.publish_key.clone(),
      subscribe_key: self.subscribe_key.clone(),
      client_uuid: self.client_uuid.clone(),
      channel: channel_c,
      group: group_c,
    };

    Subscription::new(config)
  }

  pub fn publish<T: Serialize>(&self, channel: &str, group: &str, body: T) -> PublishFuture {
    let channel_c = CString::new(channel).expect("UTF-8 doesn't include nul");
    let group_c = CString::new(group).expect("UTF-8 doesn't include nul");

    let config = ChannelConfig {
      auth_key: self.auth_key.clone(),
      publish_key: self.publish_key.clone(),
      subscribe_key: self.subscribe_key.clone(),
      client_uuid: self.client_uuid.clone(),
      channel: channel_c,
      group: group_c,
    };

    // TODO: we may want a context pool as each context consumes significant resources.
    PublishFuture::new(config, body)
  }
}

pub struct Subscription<T> {
  ctx: *mut pubnub_t,
  rx: Receiver<Result<T, ClientError>>,
  // We hold on to this so that we can free the memory later.
  user_data: *mut SubscribeUserData<T>,
  // We pass refs of these to C land.  We keep them around here so they will not be freed until the `Subscription` is dropped.
  _auth_key: CString,
  _publish_key: CString,
  _subscribe_key: CString,
  _client_uuid: CString,
  _channel: CString,
  _group: CString,
}

// The pointers in `Subscription` are thread-safe, so we can implement this.
// See https://www.pubnub.com/docs/posix-c/api-reference-configuration#Thread_safety
unsafe impl<T> Send for Subscription<T> {}

impl<'a, T: Send + Sync + Deserialize<'a>> Subscription<T> {
  fn new(config: ChannelConfig) -> Self {
    let ChannelConfig {
      auth_key,
      publish_key,
      subscribe_key,
      client_uuid,
      channel,
      group,
    } = config;

    let (tx, rx) = futures::sync::mpsc::channel::<Result<T, ClientError>>(10);

    let user_data = Box::into_raw(Box::new(SubscribeUserData {
      tx,
      channel: channel.clone(), // TODO: can this just be a reference?
    }));

    let ctx = unsafe {
      let ctx = pubnub_alloc();
      pubnub_init(ctx, publish_key.as_ptr(), subscribe_key.as_ptr());
      pubnub_set_uuid(ctx, client_uuid.as_ptr());
      pubnub_set_auth(ctx, auth_key.as_ptr());
      pubnub_register_callback(ctx, Some(subscribe_callback::<T>), user_data as *mut std::ffi::c_void);
      pubnub_subscribe(ctx, channel.as_ptr(), std::ptr::null()); // TODO: technically we shouldn't call this line until the stream gets polled the first time.
      ctx
    };

    Self {
      ctx,
      rx,
      _channel: channel,
      _auth_key: auth_key,
      _publish_key: publish_key,
      _subscribe_key: subscribe_key,
      _group: group,
      _client_uuid: client_uuid,
      user_data,
    }
  }
}

impl<T: std::fmt::Debug> Stream for Subscription<T> {
  type Item = T;
  type Error = ClientError;

  fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
     match self.rx.poll() {
        Ok(Async::Ready(Some(Ok(t)))) => Ok(Async::Ready(Some(t))),
        Ok(Async::Ready(Some(Err(e)))) => Err(e),
        Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
        Ok(Async::NotReady) => Ok(Async::NotReady),
        Err(()) => panic!("Received error from an mpsc channel, this shouldn't be possible."),
     }
  }
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

        serde_json::from_str::<T>(s).map_err(|e| ClientError::ParseError(JsonError { err: e }))
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
    // We shouldn't need to notify, because that is taken care of by the channel.
  }

  // TODO: verify that we are happy with this here.  PubNub docs suggest that it is ok to do operations like this inside of a callback, but not recommended (as it can make debugging harder).  Our use case is simple (a loop), so maybe we're ok?
  pubnub_subscribe(pb, ud.channel.as_ptr(), std::ptr::null());
}

impl<T> Drop for Subscription<T> {
  fn drop(&mut self) {
    unsafe {
      pubnub_free(self.ctx);
      Box::from_raw(self.user_data); // This should be safe because we have cleared the mutable ref held by self.ctx
    };
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

pub struct PublishFuture {
  // This would be a oneshot, but we can't get ownership of the tx end in the callback (to send the message), so we use mpsc as if it were a oneshot.
  user_data: Option<*mut PublishFutureUserData>,
  rx: Option<Receiver<Result<(), ClientError>>>,
  ctx: *mut pubnub_t,
  channel: CString,
  _auth_key: CString,
  _publish_key: CString,
  _subscribe_key: CString,
  _group: CString,
  _client_uuid: CString,
  msg: CString,
  started: bool,
}

impl PublishFuture {
  fn new<T: Serialize>(config: ChannelConfig, msg: T) -> Self {
    let msg_string = serde_json::to_string(&msg).unwrap();
    let msg_c = CString::new(msg_string).unwrap();
    let ChannelConfig {
      publish_key,
      subscribe_key,
      client_uuid,
      auth_key,
      channel,
      group,
    } = config;

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
      _auth_key: auth_key,
      _publish_key: publish_key,
      _subscribe_key: subscribe_key,
      _group: group,
      _client_uuid: client_uuid,
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
    } else if let Some(ref mut rx) = self.rx {
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

#[derive(Debug)]
pub enum ClientError {
  NullPointerError,
  ParseError(JsonError),
  PollError,
  PubNub { code: pubnub_res },
}

impl std::fmt::Display for ClientError {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    match self {
      ClientError::NullPointerError => write!(f, "PubNub client null pointer error"),
      ClientError::ParseError(e) => write!(f, "PubNub client parse error: {}", e),
      ClientError::PollError => write!(f, "PubNub client poll error"),
      ClientError::PubNub { code } => write!(f, "PubNub client error with code {}", code), // TODO: it would be nice to do these codes as an enum, but bindgen does not recommend directly building enums, as we do not own the c code.
    }
  }
}

impl std::error::Error for ClientError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    match self {
      ClientError::ParseError(e) => e.source(),
      _ => None,
    }
  }
}

#[derive(Debug)]
pub struct JsonError {
  err: serde_json::Error,
}

impl std::fmt::Display for JsonError {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    self.err.fmt(f)
  }
}

impl std::error::Error for JsonError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    self.err.source()
  }
}
