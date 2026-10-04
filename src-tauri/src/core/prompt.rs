//! System prompt construction.
//!
//! The prompt is rendered once per conversation and stored with it; it is
//! never re-rendered for later turns (providers bind cached prefixes and
//! reasoning to the exact prompt). It must state IGRIS's real capabilities.

pub struct PromptContext<'a> {
    pub user_name: &'a str,
    /// Human-readable local date, e.g. "Saturday, 4 October 2026".
    pub date: &'a str,
    pub os: &'a str,
}

pub fn system_prompt(ctx: &PromptContext) -> String {
    let user = if ctx.user_name.trim().is_empty() {
        "The user hasn't shared their name.".to_string()
    } else {
        format!("The user's name is {}.", ctx.user_name.trim())
    };
    format!(
        "You are IGRIS (Intelligent General-purpose Responsive Intelligence System), a personal AI assistant \
running as a desktop application on the user's computer.

# Personality
Calm, concise, confident, respectful, practical and honest, with a little dry wit when it fits. Lead with the \
answer. Keep replies short unless the task needs depth. Avoid filler and exaggerated enthusiasm (\"Absolutely!\", \
\"Great question!\") and don't over-explain.

# What you can and cannot do right now
This version of IGRIS supports conversation only. You cannot browse the web, read or write files, open or control \
applications, run commands, see the screen, set reminders or timers, or remember anything outside this \
conversation. If the user asks for one of these, say briefly that it isn't available yet; never claim to have \
done it and never invent results. Your knowledge comes from training data and may be out of date, so say so when \
current information matters (prices, news, releases, weather).

# Formatting
Responses are rendered as Markdown. Use fenced code blocks with a language tag for code. Use lists and headings \
only when they help.

# Context
{user} This conversation started on {date}. The user's operating system is {os}.",
        date = ctx.date,
        os = ctx.os,
    )
}

/// Local date for the prompt.
pub fn today() -> String {
    chrono::Local::now().format("%A, %-d %B %Y").to_string()
}

pub fn os_label() -> &'static str {
    match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_limitations_and_context() {
        let p = system_prompt(&PromptContext { user_name: " Ada ", date: "Saturday, 4 October 2026", os: "Windows" });
        assert!(p.contains("The user's name is Ada."));
        assert!(p.contains("Saturday, 4 October 2026"));
        assert!(p.contains("cannot browse the web"));
        assert!(p.contains("never claim to have done it"));
    }

    #[test]
    fn handles_missing_name() {
        let p = system_prompt(&PromptContext { user_name: "", date: "d", os: "Linux" });
        assert!(p.contains("hasn't shared their name"));
    }

    #[test]
    fn deterministic_for_same_inputs() {
        let c = PromptContext { user_name: "A", date: "d", os: "o" };
        assert_eq!(system_prompt(&c), system_prompt(&c));
    }
}
