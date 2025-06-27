use std::time::Duration;

use async_graphql::{Enum, InputObject, Object};
use rand::{
    distributions::{Alphanumeric, DistString},
    rngs::StdRng,
    SeedableRng,
};
use regex::Regex;

use super::room::RoomType;

#[derive(Enum, Copy, Clone, Eq, PartialEq, Debug)]
pub enum DispatcherType {
    Random,
}

#[derive(Clone)]
pub struct RoomDispatcher {
    pub name: String,
    pub admin_password: String,
    pub rule: Regex,
    pub room_type: RoomType,
    pub dispatcher_type: DispatcherType,
    pub timeout_sender: tokio::sync::watch::Sender<Duration>,
    pub timeout_receiver: tokio::sync::watch::Receiver<Duration>,
}

// graphql conversion
#[Object]
impl RoomDispatcher {
    async fn name(&self) -> String {
        self.name.clone()
    }

    async fn rule(&self) -> String {
        self.rule.as_str().to_string()
    }

    async fn room_type(&self) -> RoomType {
        self.room_type
    }

    async fn dispatcher_type(&self) -> DispatcherType {
        self.dispatcher_type
    }
}

#[derive(InputObject, Clone)]
pub struct RoomDispatcherInput {
    pub name: String,
    pub admin_password: Option<String>,
    pub rule: String,
    pub room_type: RoomType,
    pub dispatcher_type: DispatcherType,
    pub timeout: u64,
}

impl From<RoomDispatcherInput> for RoomDispatcher {
    fn from(value: RoomDispatcherInput) -> Self {
        let (timeout_sender, timeout_receiver) =
            tokio::sync::watch::channel(Duration::from_secs(value.timeout));
        RoomDispatcher {
            name: value.name,
            admin_password: if let Some(pw) = value.admin_password {
                pw
            } else {
                Alphanumeric.sample_string(&mut StdRng::from_entropy(), 8)
            },
            rule: Regex::new(&value.rule).unwrap(),
            room_type: value.room_type,
            dispatcher_type: value.dispatcher_type,
            timeout_sender,
            timeout_receiver,
        }
    }
}
