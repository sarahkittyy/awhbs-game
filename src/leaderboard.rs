//! Online leaderboard: a run token from the server when a game starts, score submission from the
//! death screen, and the board shown on the title and death screens. Requests go through `ehttp`
//! (`fetch` in the browser, a thread on desktop); replies land in an inbox polled every frame.
//!
//! Players are a random id kept in local storage, so "your place" follows the browser (or the
//! desktop install) without accounts. The server keeps each player's best run.

use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use serde::Deserialize;
use serde_json::json;

use crate::{storage, GameState, RunBegin, RunClock};

pub const NAME_MAX: usize = 16;

/// Characters a name may use (the server checks the same set).
pub fn name_char_ok(c: char) -> bool {
    c.is_ascii_alphanumeric() || " _-.!?'".contains(c)
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Entry {
    pub rank: u32,
    pub name: String,
    pub time_ms: u32,
    #[serde(default)]
    pub you: bool,
}

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Board {
    pub top: Vec<Entry>,
    pub me: Option<Entry>,
    pub total: u32,
}

/// Latest board, or why there is none.
#[derive(Resource, Default)]
pub struct Leaderboard {
    pub board: Option<Board>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubmitState {
    Idle,
    Sending,
    Done { run_rank: u32, best: bool },
    Failed(String),
}

/// This run's token and how its submission is going.
#[derive(Resource)]
pub struct RunSubmit {
    token: Option<String>,
    pub state: SubmitState,
}

/// Who this browser / install is on the board.
#[derive(Resource)]
pub struct Identity {
    pub id: String,
    pub name: String,
}

impl Identity {
    fn load() -> Identity {
        let id = storage::get("player_id")
            .filter(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .unwrap_or_else(|| {
                let id: String = (0..32).map(|_| char::from_digit(fastrand::u32(0..16), 16).unwrap()).collect();
                storage::set("player_id", &id);
                id
            });
        let name = storage::get("name").unwrap_or_default().chars().filter(|c| name_char_ok(*c)).take(NAME_MAX).collect();
        Identity { id, name }
    }
}

/// Ask to submit the finished run under `Identity::name`.
#[derive(Message)]
pub struct SubmitScore;

enum Reply {
    Token(String),
    Board(Board),
    Submitted { run_rank: u32, best: bool, board: Board },
    BoardFailed(String),
    SubmitFailed(String),
}

#[derive(Resource, Clone, Default)]
struct Inbox(Arc<Mutex<Vec<Reply>>>);

pub struct LeaderboardPlugin;

impl Plugin for LeaderboardPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Identity::load())
            .insert_resource(RunSubmit { token: None, state: SubmitState::Idle })
            .init_resource::<Leaderboard>()
            .init_resource::<Inbox>()
            .add_message::<SubmitScore>()
            .add_systems(OnEnter(GameState::Title), fetch_board)
            .add_systems(OnEnter(GameState::Dead), fetch_board)
            .add_systems(Update, (start_run, submit, poll).chain());
    }
}

/// Where the API lives: the page's own origin on the web, the public site on desktop
/// (`AWHBS_API` overrides it, e.g. `http://127.0.0.1:3547` for a local server).
fn api(path: &str) -> String {
    #[cfg(target_arch = "wasm32")]
    let base = String::new();
    #[cfg(not(target_arch = "wasm32"))]
    let base = std::env::var("AWHBS_API")
        .ok()
        .or(option_env!("AWHBS_API").map(str::to_string))
        .unwrap_or_else(|| "https://awhbs.fluffycat.gay".to_string());
    format!("{}{path}", base.trim_end_matches('/'))
}

fn post(path: &str, body: serde_json::Value) -> ehttp::Request {
    let mut req = ehttp::Request::post(api(path), serde_json::to_vec(&body).unwrap_or_default());
    req.headers = ehttp::Headers::new(&[("Accept", "application/json"), ("Content-Type", "application/json")]);
    req
}

fn error_text(res: &ehttp::Response) -> String {
    serde_json::from_slice::<serde_json::Value>(&res.bytes)
        .ok()
        .and_then(|v| v.get("error")?.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("leaderboard unavailable ({})", res.status))
}

fn send(inbox: &Inbox, req: ehttp::Request, parse: impl FnOnce(ehttp::Result<ehttp::Response>) -> Reply + Send + 'static) {
    let inbox = inbox.clone();
    ehttp::fetch(req, move |res| inbox.0.lock().unwrap().push(parse(res)));
}

fn fetch_board(inbox: Res<Inbox>, id: Res<Identity>) {
    let req = ehttp::Request::get(api(&format!("/api/leaderboard?player={}&limit=10", id.id)));
    send(&inbox, req, |res| match res {
        Ok(r) if r.ok => serde_json::from_slice(&r.bytes)
            .map(Reply::Board)
            .unwrap_or_else(|_| Reply::BoardFailed("leaderboard unavailable".into())),
        Ok(r) => Reply::BoardFailed(error_text(&r)),
        Err(_) => Reply::BoardFailed("offline".into()),
    });
}

fn start_run(mut begin: MessageReader<RunBegin>, mut run: ResMut<RunSubmit>, inbox: Res<Inbox>) {
    if begin.read().count() == 0 {
        return;
    }
    *run = RunSubmit { token: None, state: SubmitState::Idle };
    send(&inbox, post("/api/run", json!({})), |res| match res {
        Ok(r) if r.ok => serde_json::from_slice::<serde_json::Value>(&r.bytes)
            .ok()
            .and_then(|v| v.get("token")?.as_str().map(|t| Reply::Token(t.to_string())))
            .unwrap_or_else(|| Reply::SubmitFailed("leaderboard unavailable".into())),
        // a run that couldn't get a token just can't be submitted; nothing to show yet
        _ => Reply::Token(String::new()),
    });
}

fn submit(
    mut asks: MessageReader<SubmitScore>,
    mut run: ResMut<RunSubmit>,
    id: Res<Identity>,
    clock: Res<RunClock>,
    inbox: Res<Inbox>,
) {
    if asks.read().count() == 0 || matches!(run.state, SubmitState::Sending | SubmitState::Done { .. }) {
        return;
    }
    let name = id.name.trim();
    if name.is_empty() {
        run.state = SubmitState::Failed("type a name first".into());
        return;
    }
    let Some(token) = run.token.clone().filter(|t| !t.is_empty()) else {
        run.state = SubmitState::Failed("this run wasn't registered (offline?)".into());
        return;
    };
    storage::set("name", name);
    run.state = SubmitState::Sending;
    let body = json!({ "token": token, "player_id": id.id, "name": name, "time_ms": (clock.0 * 1000.0) as u64 });
    send(&inbox, post("/api/score", body), |res| match res {
        Ok(r) if r.ok => {
            #[derive(Deserialize)]
            struct Submitted {
                run_rank: u32,
                best: bool,
                board: Board,
            }
            serde_json::from_slice::<Submitted>(&r.bytes)
                .map(|s| Reply::Submitted { run_rank: s.run_rank, best: s.best, board: s.board })
                .unwrap_or_else(|_| Reply::SubmitFailed("leaderboard unavailable".into()))
        }
        Ok(r) => Reply::SubmitFailed(error_text(&r)),
        Err(_) => Reply::SubmitFailed("couldn't reach the leaderboard".into()),
    });
}

fn poll(inbox: Res<Inbox>, mut board: ResMut<Leaderboard>, mut run: ResMut<RunSubmit>) {
    let replies: Vec<Reply> = std::mem::take(&mut *inbox.0.lock().unwrap());
    for reply in replies {
        match reply {
            Reply::Token(t) => run.token = Some(t),
            Reply::Board(b) => {
                board.board = Some(b);
                board.error = None;
            }
            Reply::BoardFailed(e) => {
                if board.board.is_none() {
                    board.error = Some(e);
                }
            }
            Reply::Submitted { run_rank, best, board: b } => {
                run.state = SubmitState::Done { run_rank, best };
                board.board = Some(b);
                board.error = None;
            }
            Reply::SubmitFailed(e) => run.state = SubmitState::Failed(e),
        }
    }
}

/// `mm:ss` for a time in milliseconds.
pub fn mmss(ms: u32) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}", s / 60, s % 60)
}
