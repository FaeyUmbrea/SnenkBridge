use std::{
    fmt::{Display, Error, Formatter},
    sync::{atomic::AtomicBool, mpsc::Sender, Arc},
};

use log::warn;

use serde::{Deserialize, Serialize};

use crate::tracking::response::TrackingResponse;

pub trait TrackingClient {
    fn run(ip: String, sender: Sender<TrackingResponse>, active: Arc<AtomicBool>);

    // Something like middleware
    fn send(sender: &Sender<TrackingResponse>, response: TrackingResponse) {
        if let Err(error) = sender.send(response) {
            warn!("Unable to send tracking response: {:?}", error);
        }
    }
}

#[derive(PartialEq, Debug, Clone, Serialize, Deserialize, Default)]
pub enum TrackingClientType {
    #[default]
    VTubeStudio,
    IFacialMocap,
}

impl Display for TrackingClientType {
    fn fmt(&self, f: &mut Formatter) -> Result<(), Error> {
        match self {
            TrackingClientType::VTubeStudio => write!(f, "VTubeStudio"),
            TrackingClientType::IFacialMocap => write!(f, "iFacialMocap"),
        }
    }
}
