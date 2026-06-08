use crate::models::room::{BroadcastRoom, RoomType};
use async_graphql::futures_util::{stream, StreamExt};
use async_graphql::{Enum, InputObject, Object};
use rand::distributions::{Alphanumeric, DistString};
use rand::prelude::{SliceRandom, StdRng};
use rand::SeedableRng;
use regex::Regex;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

#[derive(Enum, Copy, Clone, Eq, PartialEq, Debug)]
pub enum DispatcherType {
    Random,
    NextFreeAlphabetical,
    NextFreeRandom,
}

impl TryFrom<String> for DispatcherType {
    type Error = ();

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.to_ascii_lowercase().as_str() {
            "random" => Ok(DispatcherType::Random),
            "nextfreealpha" => Ok(DispatcherType::NextFreeAlphabetical),
            "nextfreerandom" => Ok(DispatcherType::NextFreeRandom),
            _ => Err(()),
        }
    }
}

impl DispatcherType {
    pub async fn choose_room(&self, rooms: Vec<Arc<BroadcastRoom>>) -> Option<Arc<BroadcastRoom>> {
        let mut empty_rooms: Vec<(String, Arc<BroadcastRoom>)> = stream::iter(rooms.clone())
            .then(|room| async move {
                let (listeners, name) = {
                    let listeners = *room.meta().num_listeners.borrow();
                    let name = room.meta().name.clone();
                    (listeners, name)
                };
                (listeners <= 0, name, room)
            })
            .filter(|(ok, _name, _room)| futures::future::ready(*ok))
            .map(|(_ok, name, room)| (name, room))
            .collect()
            .await;

        match self {
            DispatcherType::Random => rooms.choose(&mut StdRng::from_entropy()).cloned(),
            DispatcherType::NextFreeAlphabetical => {
                empty_rooms.sort_by(|a, b| a.0.cmp(&b.0));
                empty_rooms.first().map(|(_, room)| room.clone())
            }
            DispatcherType::NextFreeRandom => empty_rooms
                .choose(&mut StdRng::from_entropy())
                .map(|(_, room)| room.clone()),
        }
    }
}

/// A dispatcher allows to select a room based on a given
/// regular expression and a given dispatcher_type.
#[derive(Clone)]
pub struct RoomDispatcher {
    /// name of the dispatcher - used for identification and must be unique
    pub name: String,
    /// only people with the password can modify the dispatcher until it is deleted
    /// or it is timed out.
    pub admin_password: String,
    /// each room which matches this regex will be considered a candidate
    pub rule: Regex,
    /// determines which dispatcher rule to apply on the filtered candidates
    pub dispatcher_type: DispatcherType,
    pub timeout_sender: tokio::sync::watch::Sender<Duration>,
    pub timeout_receiver: tokio::sync::watch::Receiver<Duration>,
    /// if set, the stream website will also create an audio back channel
    /// with the given prefix.
    pub return_room_prefix: Option<String>,
    /// if true there will also be a random string added postfix to the name
    /// of the back channel. this allows to have many people consuming an
    /// channel via a dispatcher but still receive the back-channel
    /// of each listener.
    pub add_random_postfix: bool,
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

    async fn dispatcher_type(&self) -> DispatcherType {
        self.dispatcher_type
    }

    async fn return_room_prefix(&self) -> Option<String> {
        self.return_room_prefix.clone()
    }

    async fn append_random_postfix(&self) -> bool {
        self.add_random_postfix
    }
}

impl From<RoomDispatcherInput> for RoomDispatcher {
    fn from(value: RoomDispatcherInput) -> Self {
        let (timeout_sender, timeout_receiver) =
            tokio::sync::watch::channel(Duration::from_secs(value.timeout.try_into().unwrap()));
        RoomDispatcher {
            name: value.name,
            admin_password: if let Some(pw) = value.admin_password {
                pw
            } else {
                Alphanumeric.sample_string(&mut StdRng::from_entropy(), 8)
            },
            rule: Regex::new(&value.rule).unwrap(),
            dispatcher_type: value.dispatcher_type,
            timeout_sender,
            timeout_receiver,
            return_room_prefix: value.return_room_prefix,
            add_random_postfix: value.add_random_postfix,
        }
    }
}

#[derive(InputObject, Clone)]
pub struct RoomDispatcherInput {
    pub name: String,
    pub admin_password: Option<String>,
    pub rule: String,
    pub room_type: RoomType,
    pub dispatcher_type: DispatcherType,
    pub timeout: i32,
    pub return_room_prefix: Option<String>,
    pub add_random_postfix: bool,
}
