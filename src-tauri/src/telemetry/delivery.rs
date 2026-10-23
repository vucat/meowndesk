//! Acknowledged single-window Channel. One in-flight message and one replaceable pending View.
use crate::telemetry::{
    model::*,
    state::{CommandError, CommandErrorCode},
};
use std::sync::Arc;
pub const MAX_DELIVERY_BYTES: usize = 256 * 1024;
pub type Sender = Arc<dyn Fn(Delivery) -> Result<(), ()> + Send + Sync>;
struct Subscription {
    id: String,
    sender: Sender,
    started: bool,
    next: u64,
    in_flight: Option<u64>,
    pending: Option<View>,
}
#[derive(Default)]
pub struct DeliveryState {
    subscription: Option<Subscription>,
}
impl DeliveryState {
    pub fn subscribe(&mut self, sender: Sender) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.subscription = Some(Subscription {
            id: id.clone(),
            sender,
            started: false,
            next: 1,
            in_flight: None,
            pending: None,
        });
        id
    }
    pub fn unsubscribe(&mut self, id: &str) -> bool {
        if self.subscription.as_ref().is_some_and(|s| s.id == id) {
            self.subscription = None;
            true
        } else {
            false
        }
    }
    pub fn clear(&mut self) {
        self.subscription = None;
    }
    pub fn clear_pending(&mut self) {
        if let Some(s) = self.subscription.as_mut() {
            s.pending = None;
        }
    }
    pub fn counts(&self) -> (usize, usize) {
        self.subscription.as_ref().map_or((0, 0), |s| {
            (
                usize::from(s.in_flight.is_some()),
                usize::from(s.pending.is_some()),
            )
        })
    }
    pub fn offer(&mut self, view: View, allowed: bool) -> Result<(), CommandError> {
        if !allowed {
            self.clear_pending();
            return Ok(());
        }
        let Some(s) = self.subscription.as_mut() else {
            return Ok(());
        };
        if s.started && s.in_flight.is_none() {
            self.send(view)
        } else {
            s.pending = Some(view);
            Ok(())
        }
    }
    pub fn ack(
        &mut self,
        id: &str,
        sequence: u64,
        latest: &View,
        allowed: bool,
    ) -> Result<bool, CommandError> {
        let Some(s) = self.subscription.as_mut() else {
            return Ok(false);
        };
        if s.id != id {
            return Err(CommandError::new(
                CommandErrorCode::InvalidSubscription,
                "The subscription has been replaced",
            ));
        }
        if sequence == 0 {
            if s.started {
                return Ok(false);
            }
            s.started = true;
            s.pending = None;
            if allowed {
                self.send(latest.clone())?;
            }
            return Ok(true);
        }
        if s.in_flight != Some(sequence) {
            return Ok(false);
        }
        s.in_flight = None;
        let pending = s.pending.take();
        if allowed && let Some(pending) = pending {
            self.send(pending)?;
        }
        Ok(true)
    }
    fn send(&mut self, view: View) -> Result<(), CommandError> {
        let Some(s) = self.subscription.as_mut() else {
            return Ok(());
        };
        let message = Delivery {
            contract_version: CONTRACT_VERSION,
            session_id: view.session_id.clone(),
            subscription_id: s.id.clone(),
            delivery_sequence: s.next,
            view,
        };
        validate_payload(&message)?;
        // Record ownership before posting. Neither collection nor this owner waits for frontend ACKs.
        s.in_flight = Some(s.next);
        s.next = s
            .next
            .checked_add(1)
            .filter(|v| *v <= MAX_JSON_SAFE_INT)
            .ok_or_else(|| {
                CommandError::new(
                    CommandErrorCode::Internal,
                    "The delivery session must be restarted",
                )
            })?;
        if (s.sender)(message).is_err() {
            self.subscription = None;
            return Err(CommandError::new(
                CommandErrorCode::InvalidSubscription,
                "The telemetry channel closed",
            ));
        }
        Ok(())
    }
}
pub fn validate_payload(message: &Delivery) -> Result<(), CommandError> {
    message.validate().map_err(|_| {
        CommandError::new(
            CommandErrorCode::Internal,
            "The telemetry view could not be validated",
        )
    })?;
    let bytes = serde_json::to_vec(message).map_err(|_| {
        CommandError::new(
            CommandErrorCode::Internal,
            "The telemetry view could not be serialized",
        )
    })?;
    if bytes.len() > MAX_DELIVERY_BYTES {
        return Err(CommandError::new(
            CommandErrorCode::PayloadTooLarge,
            "The telemetry view exceeded its delivery limit",
        ));
    }
    Ok(())
}
