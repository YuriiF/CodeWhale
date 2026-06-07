//! Slash command input parsing helpers.

pub(super) struct ParsedCommand<'a> {
    pub(super) name: String,
    pub(super) arg: Option<&'a str>,
}

pub(super) fn parse_slash_command(cmd: &str) -> ParsedCommand<'_> {
    let trimmed = cmd.trim();
    let mut parts = trimmed.splitn(2, ' ');
    let raw_command = parts.next().unwrap_or_default();
    let name = raw_command
        .strip_prefix('/')
        .unwrap_or(raw_command)
        .to_lowercase();
    let arg = parts.next().map(str::trim);

    ParsedCommand { name, arg }
}
