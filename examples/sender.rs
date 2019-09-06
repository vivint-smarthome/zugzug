use futures::Future;
use zugzug::*;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;

#[derive(StructOpt)]
struct Opt {
  #[structopt(short = "c", long = "channel", env = "PN_CHANNEL")]
  channel: String,
  #[structopt(short = "a", long = "auth_key", env = "PN_AUTH_KEY")]
  auth_key: String,
  #[structopt(short = "p", long = "publish_key", env = "PN_PUBLISH_KEY")]
  publish_key: String,
  #[structopt(short = "s", long = "subscribe_key", env = "PN_SUBSCRIBE_KEY")]
  subscribe_key: String,
  #[structopt(short = "g", long = "group", env = "PN_GROUP")]
  group: String,
  #[structopt(short = "u", long = "client_uuid", env = "PN_CLIENT_UUID")]
  client_uuid: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Stuff {
  message: String,
}

fn main() {
  let opt = Opt::from_args();
  let mut client = PublishClient::new(ClientConfig {
    auth_key: opt.auth_key,
    publish_key: opt.publish_key,
    subscribe_key: opt.subscribe_key,
    channel: opt.channel,
    group: opt.group,
    client_uuid: opt.client_uuid,
  })
  .unwrap();

  let mut i = 0;
  loop {
    println!("sending");
    client
      .publish(Stuff {
        message: format!("#{}", i),
      })
      .wait()
      .map_err(|e| println!("send error {:?}", e))
      .ok();
    std::thread::sleep(std::time::Duration::from_millis(1000));
    i += 1;
  }
}
