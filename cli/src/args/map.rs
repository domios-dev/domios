use clap::Args;

#[derive(Args, Debug, Clone)]
pub struct MapArgs {
    /// Substring filter for identifier names. Tokens are split on whitespace
    /// and matched case-insensitively; a tag scores once per matching token.
    #[arg(long)]
    pub query: Option<String>,

    /// Comma-separated list of subpaths to scan, relative to the workspace
    /// root (cwd). Defaults to the whole cwd.
    #[arg(long, value_delimiter = ',')]
    pub paths: Vec<String>,

    /// Approximate token budget for the rendered output (estimated as
    /// bytes / 4). Ignored when `--signatures` is set.
    #[arg(long, default_value_t = 4000)]
    pub max_tokens: usize,

    /// Single-file mode — dump every signature in this file. Bypasses
    /// `--query` and `--max-tokens`.
    #[arg(long, value_name = "PATH")]
    pub signatures: Option<String>,

    /// Invalidate cache before scanning.
    #[arg(long)]
    pub refresh: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: MapArgs,
    }

    #[test]
    fn defaults() {
        let cli = TestCli::try_parse_from(["x"]).expect("defaults parse");
        assert!(cli.args.query.is_none());
        assert!(cli.args.paths.is_empty());
        assert_eq!(cli.args.max_tokens, 4000);
        assert!(cli.args.signatures.is_none());
        assert!(!cli.args.refresh);
    }

    #[test]
    fn paths_split_on_comma() {
        let cli = TestCli::try_parse_from(["x", "--paths", "src/api,src/db"])
            .expect("paths parse");
        assert_eq!(cli.args.paths, vec!["src/api", "src/db"]);
    }

    #[test]
    fn query_and_max_tokens() {
        let cli = TestCli::try_parse_from([
            "x", "--query", "auth login", "--max-tokens", "1000",
        ])
        .expect("flags parse");
        assert_eq!(cli.args.query.as_deref(), Some("auth login"));
        assert_eq!(cli.args.max_tokens, 1000);
    }

    #[test]
    fn signatures_path() {
        let cli = TestCli::try_parse_from(["x", "--signatures", "src/lib.rs"])
            .expect("signatures parse");
        assert_eq!(cli.args.signatures.as_deref(), Some("src/lib.rs"));
    }

    #[test]
    fn refresh_flag() {
        let cli = TestCli::try_parse_from(["x", "--refresh"]).expect("refresh");
        assert!(cli.args.refresh);
    }
}
