//! Synthetic scenario mode: generated chats and a fake engine for visual,
//! input and performance checks. It never starts or contacts a real engine,
//! and nothing it produces is persisted.
//!
//! The generator is deterministic: message `n` always has the same content,
//! so checks can compute the exact text they expect to copy.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bukno_core::event::{EngineEvent, EngineEventKind, Input};
use bukno_core::ids::{ItemId, RunId, TaskId};
use bukno_core::message::{ItemKind, Provider, TranscriptItem};
use bukno_core::run::RunOutcome;
use tokio::sync::mpsc;

/// The single chat every synthetic scenario uses.
pub const TASK: TaskId = TaskId(0x5b5e_0000_0000_0000_0000_0000_0000_0001);
const ITEM_BASE: u128 = 0x5b5e_0000_0000_0000_0001_0000_0000_0000;
/// Connection generation of the synthetic engine. It never restarts.
pub const GENERATION: u64 = 1;

#[derive(Clone, Debug)]
pub struct Scenario {
    pub name: &'static str,
    pub title: &'static str,
    /// Number of generated history messages, alternating user and Codex.
    pub messages: usize,
    /// Submit this message on launch so a reply streams into the active block.
    pub auto_submit: Option<String>,
    /// Words in each synthetic reply.
    pub reply_words: usize,
    /// Delay between streamed words.
    pub word_interval: Duration,
}

impl Scenario {
    /// Look up a scenario by its command-line name.
    pub fn named(name: &str) -> Option<Self> {
        let base = Self {
            name: "empty",
            title: "Synthetic scenario",
            messages: 0,
            auto_submit: None,
            reply_words: 160,
            word_interval: Duration::from_millis(30),
        };
        Some(match name {
            "empty" => base,
            "short" => Self { name: "short", title: "Polish the composer", messages: 6, ..base },
            "long-chat" => {
                Self { name: "long-chat", title: "Långa chatten med 2 000 meddelanden", messages: 2_000, ..base }
            }
            "streaming" => Self {
                name: "streaming",
                title: "Långa chatten med 2 000 meddelanden",
                messages: 2_000,
                auto_submit: Some("Fortsätt skriva tills jag säger stopp.".into()),
                reply_words: 1_000_000,
                ..base
            },
            "working" => Self {
                name: "working",
                title: "Polish the composer",
                messages: 6,
                auto_submit: Some("Kör kontrollen och berätta vad du hittar.".into()),
                // Long enough to measure the working indicator alone.
                reply_words: 0,
                ..base
            },
            _ => return None,
        })
    }

    pub fn history(&self) -> Vec<TranscriptItem> {
        (1..=self.messages).map(message).collect()
    }
}

pub const SCENARIO_NAMES: &[&str] = &["empty", "short", "long-chat", "streaming", "working"];

/// One generated block of content, kept structural so its Markdown source
/// and its plain rendered text can both be derived without a parser.
#[derive(Clone, Debug, PartialEq)]
pub enum GenBlock {
    Paragraph(Vec<Inline>),
    Heading(String),
    List(Vec<String>),
    Code { lang: &'static str, code: String },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text(String),
    Code(String),
    Strong(String),
}

pub fn item_id(n: usize) -> ItemId {
    ItemId(ITEM_BASE + n as u128)
}

/// Generated message `n` (1-based). Odd numbers are the user, even numbers Codex.
pub fn message(n: usize) -> TranscriptItem {
    let user = n % 2 == 1;
    let blocks = blocks(n);
    TranscriptItem {
        id: item_id(n),
        task: TASK,
        run: None,
        kind: if user { ItemKind::UserMessage } else { ItemKind::AgentMessage { provider: Provider::Codex } },
        text: if user { plain_blocks(&blocks).join("\n\n") } else { markdown(&blocks) },
        meta: (!user).then(|| "Synthetic model · Medium".to_owned()),
        completed: true,
        revision: 1,
    }
}

/// The structural content of message `n`.
pub fn blocks(n: usize) -> Vec<GenBlock> {
    let mut rng = Lcg(n as u64 * 7919 + 17);
    if n % 2 == 1 {
        let prompt = USER_PROMPTS[rng.pick(USER_PROMPTS.len())];
        return vec![GenBlock::Paragraph(vec![Inline::Text(format!("{prompt} (meddelande {n})"))])];
    }

    let mut out = Vec::new();
    if n % 14 == 0 {
        out.push(GenBlock::Heading(format!("Sammanfattning för steg {n}")));
    }
    let mut first = vec![Inline::Text(format!("Meddelande {n}. "))];
    for i in 0..(2 + rng.pick(3)) {
        if i == 1 && n % 4 == 0 {
            first.push(Inline::Text("Jag ändrade ".into()));
            first.push(Inline::Code("composer.rs".into()));
            first.push(Inline::Text(" och ".into()));
            first.push(Inline::Strong("sparade utkastet".into()));
            first.push(Inline::Text(". ".into()));
        }
        first.push(Inline::Text(format!("{} ", SENTENCES[rng.pick(SENTENCES.len())])));
    }
    trim_trailing_space(&mut first);
    out.push(GenBlock::Paragraph(first));

    if n % 10 == 0 {
        out.push(GenBlock::List(vec![
            format!("Läste layouten för meddelande {n}"),
            "Justerade avståndet i kompositören".into(),
            "Kontrollerade tangentbordsflödet med å, ä och ö".into(),
        ]));
    }
    if n % 6 == 0 {
        out.push(GenBlock::Code {
            lang: "rust",
            code: format!(
                "fn check_{n}(draft: &Draft) -> bool {{\n    // Utkastet ska överleva byte av chatt.\n    let saved = draft.revision >= {n};\n    tracing::debug!(revision = draft.revision, saved, \"kontrollerade utkastet efter att ha bytt chatt och kommit tillbaka igen\");\n    saved\n}}"
            ),
        });
    }
    out.push(GenBlock::Paragraph(vec![Inline::Text(CLOSERS[rng.pick(CLOSERS.len())].to_owned())]));
    out
}

fn trim_trailing_space(inlines: &mut [Inline]) {
    if let Some(Inline::Text(t)) = inlines.last_mut() {
        let trimmed = t.trim_end().len();
        t.truncate(trimmed);
    }
}

pub fn markdown(blocks: &[GenBlock]) -> String {
    let mut parts = Vec::new();
    for block in blocks {
        parts.push(match block {
            GenBlock::Paragraph(inlines) => inlines
                .iter()
                .map(|i| match i {
                    Inline::Text(t) => t.clone(),
                    Inline::Code(c) => format!("`{c}`"),
                    Inline::Strong(s) => format!("**{s}**"),
                })
                .collect(),
            GenBlock::Heading(h) => format!("### {h}"),
            GenBlock::List(items) => items.iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n"),
            GenBlock::Code { lang, code } => format!("```{lang}\n{code}\n```"),
        });
    }
    parts.join("\n\n")
}

/// Plain rendered text of each block, as the transcript is expected to copy
/// it. List items are separate entries.
pub fn plain_blocks(blocks: &[GenBlock]) -> Vec<String> {
    let mut out = Vec::new();
    for block in blocks {
        match block {
            GenBlock::Paragraph(inlines) => out.push(
                inlines
                    .iter()
                    .map(|i| match i {
                        Inline::Text(t) | Inline::Code(t) | Inline::Strong(t) => t.as_str(),
                    })
                    .collect(),
            ),
            GenBlock::Heading(h) => out.push(h.clone()),
            GenBlock::List(items) => out.extend(items.iter().cloned()),
            GenBlock::Code { code, .. } => out.push(code.clone()),
        }
    }
    out
}

/// Small deterministic generator so scenarios never depend on a random seed.
struct Lcg(u64);

impl Lcg {
    fn pick(&mut self, len: usize) -> usize {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % len as u64) as usize
    }
}

const USER_PROMPTS: &[&str] = &[
    "Kan du kolla varför sidomenyn hoppar när fönstret blir smalt?",
    "Improve the composer and keep the draft when I switch chats.",
    "Gör en snabb genomgång av tangentbordsnavigeringen.",
    "Check that Swedish text like räksmörgås and Åre wraps correctly.",
    "Varför försvinner markeringen när jag scrollar långt bort?",
    "Summarise what changed in the last step.",
];

const SENTENCES: &[&str] = &[
    "Jag har läst igenom layouten och justerat avståndet i kompositören.",
    "The spacing is updated and the keyboard flow still works.",
    "Raden med ändringar visar nu +12 −3 för filen.",
    "Öppna chatten igen så ser du att utkastet finns kvar.",
    "Selection should survive scrolling far away and back again.",
    "Det här stycket är längre för att testa radbrytning över flera rader i transkriptet, med ord som räksmörgås, älgjakt, överenskommelse och självständighetsförklaring.",
    "I ran the check twice and both runs passed.",
    "Fönstret är smalt nu, så den högra panelen fälls ihop först.",
    "Nothing else in the workspace changed.",
    "Åtgärden kräver inget godkännande eftersom den bara läser filer.",
];

const CLOSERS: &[&str] = &[
    "Säg till om du vill att jag fortsätter.",
    "That is all for this step.",
    "Nästa steg är att kontrollera fokusordningen.",
    "Ready for the next direction.",
];

const REPLY_WORDS: &[&str] = &[
    "Strömmande",
    "svar",
    "från",
    "den",
    "syntetiska",
    "motorn.",
    "Varje",
    "ord",
    "kommer",
    "för",
    "sig,",
    "så",
    "markeringen",
    "måste",
    "överleva",
    "att",
    "texten",
    "växer.",
    "The",
    "active",
    "block",
    "keeps",
    "changing",
    "while",
    "you",
    "select",
    "older",
    "messages.",
    "Åäö",
    "och",
    "räksmörgås",
    "ska",
    "se",
    "rätt",
    "ut.",
];

/// The fake engine. It acknowledges a turn, streams words into one reply and
/// ends the run, or stops early when interrupted.
#[derive(Clone)]
pub struct SyntheticEngine {
    inbox: mpsc::Sender<Input>,
    scenario: Arc<Scenario>,
    running: Arc<std::sync::Mutex<Vec<(RunId, Arc<AtomicBool>)>>>,
}

impl SyntheticEngine {
    pub fn new(inbox: mpsc::Sender<Input>, scenario: Arc<Scenario>) -> Self {
        Self { inbox, scenario, running: Arc::default() }
    }

    pub fn start_turn(&self, run: RunId, reply: ItemId) {
        let stop = Arc::new(AtomicBool::new(false));
        self.running.lock().expect("synthetic engine lock").push((run, stop.clone()));
        let inbox = self.inbox.clone();
        let scenario = self.scenario.clone();
        tokio::spawn(async move {
            let send = |kind| inbox.send(Input::Engine(EngineEvent { connection_generation: GENERATION, kind }));
            tokio::time::sleep(Duration::from_millis(120)).await;
            if send(EngineEventKind::RunAccepted { run }).await.is_err() {
                return;
            }
            if scenario.reply_words == 0 {
                // "Working" without output: wait until Stop.
                while !stop.load(Ordering::Relaxed) {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
            for i in 0..scenario.reply_words {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let word = REPLY_WORDS[i % REPLY_WORDS.len()];
                // A paragraph break every 60 words keeps the active block bounded.
                let sep = if i == 0 {
                    ""
                } else if i % 60 == 0 {
                    "\n\n"
                } else {
                    " "
                };
                let delta = format!("{sep}{word}");
                if send(EngineEventKind::TextDelta { run, item: reply, delta }).await.is_err() {
                    return;
                }
                tokio::time::sleep(scenario.word_interval).await;
            }
            let outcome = if stop.load(Ordering::Relaxed) { RunOutcome::Interrupted } else { RunOutcome::Completed };
            if scenario.reply_words > 0 {
                let _ = send(EngineEventKind::ItemCompleted { run, item: reply }).await;
            }
            let _ = send(EngineEventKind::RunEnded { run, outcome }).await;
        });
    }

    pub fn interrupt(&self, run: RunId) {
        let mut running = self.running.lock().expect("synthetic engine lock");
        running.retain(|(id, stop)| {
            if *id == run {
                stop.store(true, Ordering::Relaxed);
                false
            } else {
                true
            }
        });
    }
}
