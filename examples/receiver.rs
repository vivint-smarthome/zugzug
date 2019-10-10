use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use zugzug::*;

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
  let client = Client::new(ClientConfig {
    auth_key: opt.auth_key,
    publish_key: opt.publish_key,
    subscribe_key: opt.subscribe_key,
    client_uuid: opt.client_uuid,
  });

  let channel = opt.channel;
  let group = opt.group;

  let task = client.subscribe::<Stuff>(&channel, &group);

  tokio::run(task.map_err(|e| {
    println!("An error {:?}", e);
  }).for_each(|v| {
    println!("A message! {:?}", v);
    Ok(())
  }));

  panic!("The task should never finish");
}
