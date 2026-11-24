use super::*;
use crate::bridge::traits::OutboundContent;

fn message() -> OutboundMessage {
    OutboundMessage {
        user_id: None,
        content: OutboundContent::Text {
            text: "opaque".into(),
        },
        append: false,
        buttons: None,
        edit: None,
        receipt: None,
        completion: None,
        route: Default::default(),
    }
}

#[test]
fn full_outbound_queue_refuses_immediately_and_makes_room_on_consumption() {
    let (sender, mut receiver) = outbound_channel();
    for _ in 0..CAPACITY {
        sender.send(message()).unwrap();
    }
    assert!(sender.send(message()).is_err());
    receiver.try_recv().unwrap();
    sender.send(message()).unwrap();
}

#[test]
fn expired_outbound_messages_are_dropped_before_network_delivery() {
    let now = Instant::now();
    let (completion, receipt) = std::sync::mpsc::sync_channel(1);
    let mut expired = message();
    expired.completion = Some(completion);
    assert!(take_at((now, expired), now + MAX_AGE).is_none());
    assert!(receipt.try_recv().unwrap().is_err());
    assert!(take_at((now, message()), now + MAX_AGE - Duration::from_millis(1)).is_some());
}

#[test]
fn recent_chat_text_keeps_only_bounded_receipts_within_the_edit_window() {
    use super::super::{
        expire_chat_states, remember_chat_state, ChatState, AUTO_EDIT_WINDOW_SECS, MAX_RECENT_CHATS,
    };
    let mut chats = std::collections::HashMap::new();
    let now = Instant::now();
    for id in 0..=MAX_RECENT_CHATS {
        remember_chat_state(
            &mut chats,
            id as i64,
            ChatState {
                message_id: id as i64,
                sent_at: now + Duration::from_millis(id as u64),
                text: "opaque".into(),
            },
        );
    }
    assert_eq!(chats.len(), MAX_RECENT_CHATS);
    assert!(!chats.contains_key(&0));
    expire_chat_states(
        &mut chats,
        now + Duration::from_secs(AUTO_EDIT_WINDOW_SECS) + Duration::from_millis(1),
    );
    assert_eq!(chats.len(), MAX_RECENT_CHATS - 1);
    assert!(!chats.contains_key(&1));
}
