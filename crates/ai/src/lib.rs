//! AI image providers for FigCraft.
//!
//! The first is **Sign in with ChatGPT**: the user signs in once in their browser (OpenID Connect
//! with PKCE, a callback on `127.0.0.1`) and FigCraft then calls OpenAI's Responses API with a
//! token that draws on their own ChatGPT plan, with no API key. It is written from OpenAI's public
//! protocol documentation (`developers.openai.com/siwc`); none of OpenAI's DevKit code, which is
//! under a noncommercial licence, is used.
//!
//! Open-source apps that run on the user's own machine register themselves while signing in
//! (`client_id=dynamic_agent_client`), so there is no client secret and nothing to apply for. A
//! remotely hosted app (the web build) is not eligible, so this crate is desktop and CLI only.
//!
//! * [`pkce`]: the verifier and challenge.
//! * [`chatgpt`]: the authorize URL, the loopback callback, the token exchange and refresh.
//! * [`store`]: where the tokens are kept on disk.
//! * [`responses`]: the image request and its reply.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod chatgpt;
mod error;
mod http;
pub mod percent;
pub mod pkce;
pub mod responses;
pub mod store;

pub use error::AiError;
