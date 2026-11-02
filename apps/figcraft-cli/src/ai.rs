//! `figcraft-cli ai`: sign in with ChatGPT and make an image with the user's own plan. A preview
//! of the provider the app will use; the sign-in is kept in the app's folder (`FIGCRAFT_AI_DIR`).

use std::process::ExitCode;
use std::time::Duration;

use figcraft_ai::store::{self, Credentials};
use figcraft_ai::{AiError, chatgpt, responses};

use crate::args::{AiCommand, AiImageArgs, AiLoginArgs};
use crate::{CliError, CliResult};

fn fail(e: AiError) -> CliError {
    CliError::Failed(e.to_string())
}

pub fn run(command: AiCommand) -> CliResult<ExitCode> {
    match command {
        AiCommand::Login(a) => login(&a),
        AiCommand::Status => status(),
        AiCommand::Logout => logout(),
        AiCommand::Models => models(),
        AiCommand::Image(a) => image(&a),
    }
}

fn login(a: &AiLoginArgs) -> CliResult<ExitCode> {
    let existing = store::load().map_err(fail)?;
    let host_id = store::host_id().map_err(fail)?;
    let no_browser = a.no_browser;
    let open = move |url: &str| {
        eprintln!("Sign in with ChatGPT in your browser. If it does not open, visit:\n\n{url}\n");
        if !no_browser && !chatgpt::open_in_browser(url) {
            eprintln!("(the browser could not be started: open the address above yourself)");
        }
        eprintln!("Waiting for you to finish in the browser…");
    };
    let creds = chatgpt::sign_in(&chatgpt::SignIn {
        app_name: "FigCraft",
        host_id: &host_id,
        existing: existing.as_ref(),
        open: &open,
        timeout: Duration::from_secs(a.timeout.clamp(10, 1800)),
    })
    .map_err(fail)?;
    store::save(&creds).map_err(fail)?;
    println!("Signed in. Client {}; the sign-in is kept in {}.", creds.client_id, store::dir().map_err(fail)?.display());
    print_grant(&creds);
    Ok(ExitCode::SUCCESS)
}

fn print_grant(c: &Credentials) {
    println!("Scopes granted: {}", c.tokens.scope.as_deref().unwrap_or("(none reported)"));
}

fn status() -> CliResult<ExitCode> {
    let Some(c) = store::load().map_err(fail)? else {
        println!("Not signed in. Run `figcraft-cli ai login`.");
        return Ok(ExitCode::from(1));
    };
    let left = (c.saved_at + c.tokens.expires_in.unwrap_or(0)).saturating_sub(store::now());
    println!("Signed in (client {}).", c.client_id);
    println!(
        "Access token: {}",
        if c.access_valid_at(store::now()) { format!("valid for about {} more minutes", left / 60) } else { "expired (refreshes on use)".into() }
    );
    println!(
        "Refresh token: {}",
        if c.tokens.refresh_token.is_some() { "kept" } else { "none: you will have to sign in again when the access token expires" }
    );
    println!("Plan use granted: {}", if c.tokens.can_use_api() { "yes" } else { "no" });
    print_grant(&c);
    Ok(ExitCode::SUCCESS)
}

fn logout() -> CliResult<ExitCode> {
    let was = store::clear().map_err(fail)?;
    println!("{}", if was { "Signed out: the sign-in kept on this machine was removed." } else { "There was no sign-in to remove." });
    println!("To revoke the app's access, use ChatGPT Settings → Usage.");
    Ok(ExitCode::SUCCESS)
}

/// A usable access token, refreshed (and the new tokens kept) when the stored one has expired.
fn fresh_token() -> CliResult<String> {
    let Some(creds) = store::load().map_err(fail)? else { return Err(fail(AiError::NotSignedIn)) };
    if creds.access_valid_at(store::now()) {
        return Ok(creds.tokens.access_token);
    }
    let Some(refresh) = creds.tokens.refresh_token.as_deref() else { return Err(fail(AiError::NotSignedIn)) };
    let tokens = chatgpt::refresh(&creds.client_id, refresh).map_err(fail)?;
    let renewed = creds.renewed(tokens);
    store::save(&renewed).map_err(fail)?;
    Ok(renewed.tokens.access_token)
}

fn models() -> CliResult<ExitCode> {
    let token = fresh_token()?;
    for m in responses::list_models(&token).map_err(fail)? {
        println!("{m}");
    }
    Ok(ExitCode::SUCCESS)
}

/// The model to drive the request: the one asked for, else the first of the plan's that suits an
/// image request.
fn pick_model(token: &str, asked: Option<&str>) -> CliResult<String> {
    if let Some(m) = asked {
        return Ok(m.to_string());
    }
    let list = responses::list_models(token).map_err(fail)?;
    ["gpt-5", "gpt-4.1", "gpt-4o"]
        .iter()
        .find_map(|p| list.iter().find(|m| m.starts_with(p)))
        .or_else(|| list.first())
        .cloned()
        .ok_or_else(|| CliError::Failed("the plan lists no models: pass --model".into()))
}

fn image(a: &AiImageArgs) -> CliResult<ExitCode> {
    let token = fresh_token()?;
    let model = pick_model(&token, a.model.as_deref())?;
    eprintln!("Asking {model} for an image…");
    let req = responses::ImageRequest { model: &model, prompt: &a.prompt, size: a.size.as_deref(), background: a.background.as_deref() };
    let bytes = responses::generate_image(&token, &req).map_err(fail)?;
    std::fs::write(&a.out, &bytes).map_err(|e| CliError::Failed(format!("could not write {}: {e}", a.out)))?;
    println!("Wrote {} ({} bytes).", a.out, bytes.len());
    Ok(ExitCode::SUCCESS)
}
