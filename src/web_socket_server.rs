use actix::prelude::*;
use actix_web_actors::ws;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::chat_server::{ChatServer, ClientMessage, Connect, Disconnect, GetUserChats};
//web_socket_server.rs
#[derive(Deserialize)]
struct IncomingMessage {
    recipient_id: Uuid,
    id_chat: Uuid,
    message: String,
}

#[derive(Message, Serialize)]
#[rtype(result = "()")]
pub struct ChatMessage {
    pub sender_id: Uuid,
    pub id_chat: Uuid,
    pub message: String,
}

pub struct WebSocketConnection {
    pub id: Uuid,
    pub hb: Instant,
    pub addr: Addr<ChatServer>,
}

impl Actor for WebSocketConnection {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // Start the heartbeat process
        self.hb(ctx);

        // Register the session with the chat server
        let addr = ctx.address();

        self.addr
            .send(Connect {
                user_id: self.id,
                addr: addr.recipient(),
            })
            .into_actor(self)
            .then(|res, act, ctx| {
                if res.is_err() {
                    println!("Failed to register with chat server.");
                    ctx.stop();
                } else {
                    // Once registered, request all chats associated with the user
                    act.addr
                        .send(GetUserChats {
                            user_id: act.id,
                        })
                        .into_actor(act)
                        .then(|res, _act, ctx| {
                            match res {
                                Ok(chats) => {
                                    let chat_list = serde_json::to_string(&chats).unwrap_or_default();
                                    ctx.text(chat_list); // Send chat list to the client
                                }
                                Err(_) => {
                                    println!("Failed to retrieve chats.");
                                }
                            }
                            fut::ready(())
                        })
                        .wait(ctx);
                }
                fut::ready(())
            })
            .wait(ctx);
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        self.addr.do_send(Disconnect { user_id: self.id });
    }
}


impl WebSocketConnection {
    pub fn hb(&self, ctx: &mut ws::WebsocketContext<Self>) {
        ctx.run_interval(Duration::from_secs(5), |act, ctx| {
            if Instant::now().duration_since(act.hb) > Duration::from_secs(10) {
                println!("WebSocket client heartbeat failed, disconnecting.");
                ctx.stop();
                return;
            }
            ctx.ping(b"");
        });
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WebSocketConnection {
    fn handle(
        &mut self,
        msg: Result<ws::Message, ws::ProtocolError>,
        ctx: &mut Self::Context,
    ) {
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                self.hb = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                self.hb = Instant::now();
            }
            Ok(ws::Message::Text(text)) => {
                // `text` is a `String` in actix-web-actors 4.x
                match serde_json::from_str::<IncomingMessage>(&text) {
                    Ok(incoming_msg) => {
                        // Send the message to the chat server
                        self.addr.do_send(ClientMessage {
                            sender_id: self.id,
                            recipient_id: incoming_msg.recipient_id,
                            id_chat: incoming_msg.id_chat,
                            message: incoming_msg.message,
                        });
                    }
                    Err(e) => {
                        println!("Failed to parse message: {}", e);
                    }
                }
            }
            Ok(ws::Message::Close(_)) => {
                ctx.stop();
            }
            Err(e) => {
                println!("WebSocket error: {}", e);
                ctx.stop();
            }
            _ => {}
        }
    }
}

impl Handler<ChatMessage> for WebSocketConnection {
    type Result = ();

    fn handle(&mut self, msg: ChatMessage, ctx: &mut ws::WebsocketContext<Self>) {
        // Send the message back to the client as JSON
        let outgoing_msg = serde_json::to_string(&msg).unwrap_or_default();
        println!("Sending message to user {}: {}", self.id, outgoing_msg); // Added logging
        ctx.text(outgoing_msg);
    }
}

