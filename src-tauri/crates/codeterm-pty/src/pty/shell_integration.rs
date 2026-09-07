//! Invisible shell integration: inject per-shell scripts so the shell emits
//! OSC 633 semantic markers (A/B prompt, C command-start, D[;exit] command-end,
//! E;<cmd>;<nonce> command line, P;Cwd) plus OSC 7 cwd. Scripts are written once
//! at startup and wired in via env at PTY creation, so the shell sources them
//! during its normal startup — no visible commands injected after spawn.
//!
//! Adapted from VS Code's shell-integration scripts (MIT). We COMPOSE with the
//! user's config (source it first, chain PROMPT_COMMAND / DEBUG trap / events)
//! rather than clobber, and a hook failure must never break shell startup.
//!
//! The per-session nonce arrives as $CODETERM_SHELL_NONCE; the script stamps it
//! onto 633;E and unsets the env. The marker consumer (OSC3) validates it to
//! reject command-line text forged by untrusted output. Command/property values
//! are escaped with VS Code's __vsc_escape_value scheme (backslash doubled,
//! `;` -> \x3b, control bytes -> \xHH) — OSC3 decodes with the same scheme.

use portable_pty::CommandBuilder;
use std::path::PathBuf;

/// Data dir where shell integration scripts live.
fn integration_dir() -> PathBuf {
    codeterm_paths::data_dir().join("shell-integration")
}

const ZSH_HOOKS: &str = include_zsh_hooks();
const BASH_RC: &str = include_bash_rc();
const FISH_CONF: &str = include_fish_conf();

/// Ensure shell integration scripts exist on disk. Called once at app startup.
pub fn ensure_scripts() {
    let dir = integration_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("Failed to create shell integration dir: {}", e);
        return;
    }

    // their startup files, then installs the OSC 633 precmd/preexec hooks last.
    let zshrc = dir.join(".zshrc");
    let zshrc_content = format!(
        r#"# CodeTerm shell integration — do not edit
if [[ -n "$CODETERM_ORIG_ZDOTDIR" ]]; then
  ZDOTDIR="$CODETERM_ORIG_ZDOTDIR"
  unset CODETERM_ORIG_ZDOTDIR
else
  ZDOTDIR="$HOME"
fi

[[ -f "$ZDOTDIR/.zshenv" ]]   && source "$ZDOTDIR/.zshenv"
[[ -f "$ZDOTDIR/.zprofile" ]] && source "$ZDOTDIR/.zprofile"
[[ -f "$ZDOTDIR/.zshrc" ]]    && source "$ZDOTDIR/.zshrc"

{ZSH_HOOKS}
"#
    );
    let _ = std::fs::write(&zshrc, zshrc_content);

    // .zshenv: pass through to the user's .zshenv (covers non-interactive shells).
    let zshenv = dir.join(".zshenv");
    let zshenv_content = r#"# CodeTerm — pass through to user's .zshenv
if [[ -n "$CODETERM_ORIG_ZDOTDIR" ]]; then
  [[ -f "$CODETERM_ORIG_ZDOTDIR/.zshenv" ]] && source "$CODETERM_ORIG_ZDOTDIR/.zshenv"
elif [[ -f "$HOME/.zshenv" ]]; then
  source "$HOME/.zshenv"
fi
"#;
    let _ = std::fs::write(&zshenv, zshenv_content);

    // bash: sourced via --rcfile (interactive) / BASH_ENV (non-interactive).
    // Sources ~/.bashrc first, then installs the hooks last so they survive.
    let bashrc = dir.join("bashrc");
    let _ = std::fs::write(&bashrc, BASH_RC);

    let powershell = dir.join("powershell.ps1");
    let powershell_content = r#"# CodeTerm shell integration - do not edit
if (-not $global:__CodeTermShellIntegrationLoaded) {
  $global:__CodeTermShellIntegrationLoaded = $true
  $global:__CodeTermPromptSeen = $false
  $global:__CodeTermShellNonce = $env:CODETERM_SHELL_NONCE
  Remove-Item Env:CODETERM_SHELL_NONCE -ErrorAction SilentlyContinue
  $global:__CodeTermOriginalPrompt = $function:prompt
  if ($null -eq $global:__CodeTermOriginalPrompt) {
    $global:__CodeTermOriginalPrompt = { "PS $($executionContext.SessionState.Path.CurrentLocation)> " }
  }

  function global:prompt {
    $commandFinished = $global:__CodeTermPromptSeen
    $global:__CodeTermPromptSeen = $true
    if ($commandFinished -and -not [string]::IsNullOrEmpty($global:__CodeTermShellNonce)) {
      [Console]::Write(([char]27 + ']633;E;;' + $global:__CodeTermShellNonce + [char]7))
      [Console]::Write(([char]27 + ']633;D' + [char]7))
    }
    [Console]::Write(([char]27 + ']633;A' + [char]7))
    $rendered = & $global:__CodeTermOriginalPrompt
    [Console]::Write(([char]27 + ']633;B' + [char]7))
    $rendered
  }
}
"#;
    let _ = std::fs::write(&powershell, powershell_content);

    // fish: auto-sourced from $XDG_DATA_DIRS/fish/vendor_conf.d on interactive start.
    let fish_dir = dir.join("fish").join("vendor_conf.d");
    if let Err(e) = std::fs::create_dir_all(&fish_dir) {
        log::warn!("Failed to create fish vendor_conf.d dir: {}", e);
    } else {
        let _ = std::fs::write(fish_dir.join("00-codeterm.fish"), FISH_CONF);
    }

    log::info!("Shell integration scripts ready at {}", dir.display());
}

/// Apply shell integration wiring (env + args) to a CommandBuilder for the shell.
pub fn apply_env(cmd: &mut CommandBuilder, shell: &str) {
    let dir = integration_dir();

    #[cfg(not(target_os = "windows"))]
    {
        if shell.contains("zsh") {
            let orig = std::env::var("ZDOTDIR").unwrap_or_default();
            if !orig.is_empty() {
                cmd.env("CODETERM_ORIG_ZDOTDIR", &orig);
            } else {
                cmd.env(
                    "CODETERM_ORIG_ZDOTDIR",
                    std::env::var("HOME").unwrap_or_else(|_| "/".to_string()),
                );
            }
            cmd.env("ZDOTDIR", dir.to_string_lossy().as_ref());
        } else if shell.contains("bash") {
            let bashrc_path = dir.join("bashrc");
            if bashrc_path.exists() {
                if !cmd.is_default_prog() {
                    // --rcfile drives interactive non-login bash.
                    cmd.arg("--rcfile");
                    cmd.arg(bashrc_path.to_string_lossy().as_ref());
                }
                // BASH_ENV covers non-interactive subshells (and the login shell).
                cmd.env("BASH_ENV", bashrc_path.to_string_lossy().as_ref());
            }
        } else if shell.contains("fish") {
            // Prepend our dir so fish picks up fish/vendor_conf.d/00-codeterm.fish.
            let existing = std::env::var("XDG_DATA_DIRS")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
            cmd.env(
                "XDG_DATA_DIRS",
                format!("{}:{}", dir.to_string_lossy(), existing),
            );
        } else {
            // Other POSIX shells: best-effort OSC 7 cwd only.
            cmd.env(
                "PROMPT_COMMAND",
                r#"printf '\033]7;file://%s%s\033\\' "$(hostname)" "$PWD""#,
            );
        }
    }

    #[cfg(target_os = "windows")]
    {
        let shell_name = std::path::Path::new(shell)
            .file_stem()
            .and_then(|name| name.to_str());
        if shell_name.is_some_and(|name| {
            name.eq_ignore_ascii_case("pwsh") || name.eq_ignore_ascii_case("powershell")
        }) {
            let script = dir.join("powershell.ps1");
            if script.exists() {
                cmd.arg("-NoExit");
                cmd.arg("-File");
                cmd.arg(script.as_os_str());
            }
        }
    }
}

/// zsh OSC 633 hooks (precmd/preexec + PS1 wrap). Appended after user config.
const fn include_zsh_hooks() -> &'static str {
    r####"# CodeTerm OSC 633 shell integration
if [[ -z "$CODETERM_SHELL_INTEGRATION" ]]; then
  CODETERM_SHELL_INTEGRATION=1
  __ct_nonce="$CODETERM_SHELL_NONCE"; unset CODETERM_SHELL_NONCE

  __ct_escape_value() {
    builtin emulate -L zsh
    builtin local LC_ALL=C str="$1" i byte token out='' val
    for (( i = 0; i < ${#str}; ++i )); do
      byte="${str:$i:1}"
      val=$(builtin printf '%d' "'$byte")
      if (( val < 31 )); then token=$(builtin printf '\\\\x%02x' "'$byte")
      elif [ "$byte" = "\\" ]; then token="\\\\"
      elif [ "$byte" = ";" ]; then token="\\x3b"
      else token="$byte"; fi
      out+="$token"
    done
    builtin print -r -- "$out"
  }
  __ct_osc7() { builtin printf '\033]7;file://%s%s\033\\' "${HOST:-$(hostname)}" "$PWD"; }
  __ct_in_cmd="1"; __ct_current_command=""
  __ct_prompt_start() { builtin printf '\e]633;A\a'; }
  __ct_prompt_end()   { builtin printf '\e]633;B\a'; }
  __ct_update_cwd() {
    builtin printf '\e]633;P;Cwd=%s\a' "$(__ct_escape_value "$PWD")"
    __ct_osc7
  }
  __ct_output_start() {
    builtin printf '\e]633;E;%s;%s\a' "$(__ct_escape_value "$__ct_current_command")" "$__ct_nonce"
    builtin printf '\e]633;C\a'
  }
  __ct_command_complete() {
    if [[ -z "$__ct_current_command" ]]; then builtin printf '\e]633;D\a'
    else builtin printf '\e]633;D;%s\a' "$__ct_status"; fi
    __ct_update_cwd
  }
  __ct_update_prompt() {
    __ct_prior_ps1="$PS1"
    __ct_in_cmd=""
    PS1="%{$(__ct_prompt_start)%}$PS1%{$(__ct_prompt_end)%}"
  }
  __ct_precmd() {
    __ct_status="$?"
    if [[ -z "$__ct_in_cmd" ]]; then __ct_output_start; fi
    __ct_command_complete
    __ct_current_command=""
    if [[ -n "$__ct_in_cmd" ]]; then __ct_update_prompt; fi
  }
  __ct_preexec() {
    PS1="$__ct_prior_ps1"
    __ct_in_cmd="1"
    __ct_current_command="$1"
    __ct_output_start
  }
  builtin autoload -Uz add-zsh-hook 2>/dev/null
  add-zsh-hook precmd __ct_precmd
  add-zsh-hook preexec __ct_preexec
fi
"####
}

/// bash OSC 633 rc: sources ~/.bashrc, then installs DEBUG-trap preexec +
/// PROMPT_COMMAND precmd + PS1 wrap (macOS ships bash 3.2 which lacks PS0; the
/// DEBUG trap is the portable preexec and composes with any existing trap).
const fn include_bash_rc() -> &'static str {
    r####"# CodeTerm shell integration — do not edit
[ -r ~/.bashrc ] && . ~/.bashrc

if [ -z "${CODETERM_SHELL_INTEGRATION:-}" ]; then
  CODETERM_SHELL_INTEGRATION=1
  __ct_nonce="$CODETERM_SHELL_NONCE"; unset CODETERM_SHELL_NONCE

  __ct_escape_value() {
    builtin local LC_ALL=C str="${1}" i val byte token out=''
    for (( i=0; i < "${#str}"; ++i )); do
      byte="${str:$i:1}"
      builtin printf -v val '%d' "'$byte"
      if (( val < 31 )); then builtin printf -v token '\\x%02x' "'$byte"
      elif (( val == 92 )); then token="\\\\"
      elif (( val == 59 )); then token="\\x3b"
      else token="$byte"; fi
      out+="$token"
    done
    builtin printf '%s' "$out"
  }
  __ct_osc7() { builtin printf '\033]7;file://%s%s\033\\' "$(hostname)" "$PWD"; }
  __ct_in_command_execution="1"
  __ct_current_command=""
  __ct_prompt_start() { builtin printf '\e]633;A\a'; }
  __ct_prompt_end()   { builtin printf '\e]633;B\a'; }
  __ct_update_cwd()   { builtin printf '\e]633;P;Cwd=%s\a' "$(__ct_escape_value "$PWD")"; __ct_osc7; }
  __ct_output_start() {
    if [ -z "${__ct_first_prompt-}" ]; then return; fi
    builtin printf '\e]633;E;%s;%s\a' "$(__ct_escape_value "${__ct_current_command}")" "$__ct_nonce"
    builtin printf '\e]633;C\a'
  }
  __ct_command_complete() {
    if [ -z "${__ct_first_prompt-}" ]; then __ct_update_cwd; return; fi
    if [ "$__ct_current_command" = "" ]; then builtin printf '\e]633;D\a'
    else builtin printf '\e]633;D;%s\a' "$__ct_status"; fi
    __ct_update_cwd
  }
  __ct_update_prompt() {
    if [ "$__ct_in_command_execution" = "1" ]; then
      if [ "${__ct_custom_PS1:-}" = "" ] || [ "${__ct_custom_PS1:-}" != "$PS1" ]; then
        __ct_original_PS1="$PS1"
        __ct_custom_PS1="\[$(__ct_prompt_start)\]$__ct_original_PS1\[$(__ct_prompt_end)\]"
        PS1="$__ct_custom_PS1"
      fi
      __ct_in_command_execution="0"
    fi
  }
  __ct_precmd() {
    __ct_command_complete "$__ct_status"
    __ct_current_command=""
    __ct_first_prompt=1
    __ct_update_prompt
  }
  __ct_preexec() {
    if [[ ! "$BASH_COMMAND" == __ct_* ]]; then
      __ct_current_command="$BASH_COMMAND"
    else
      __ct_current_command=""
    fi
    __ct_output_start
  }
  __ct_get_trap() {
    builtin local -a terms
    builtin eval "terms=( $(trap -p "${1:-DEBUG}") )"
    builtin printf '%s' "${terms[2]:-}"
  }
  __ct_dbg_trap="$(__ct_get_trap DEBUG)"
  if [ -z "$__ct_dbg_trap" ]; then
    __ct_preexec_only() {
      if [ "$__ct_in_command_execution" = "0" ]; then
        __ct_in_command_execution="1"
        __ct_preexec
      fi
    }
    trap '__ct_preexec_only "$_"' DEBUG
  elif [[ "$__ct_dbg_trap" != '__ct_preexec'* ]]; then
    __ct_preexec_all() {
      if [ "$__ct_in_command_execution" = "0" ]; then
        __ct_in_command_execution="1"
        __ct_preexec
        builtin eval "${__ct_dbg_trap}"
      fi
    }
    trap '__ct_preexec_all "$_"' DEBUG
  fi
  __ct_update_prompt
  __ct_restore_exit_code() { return "$1"; }
  __ct_prompt_cmd_original() {
    __ct_status="$?"
    builtin local cmd
    __ct_restore_exit_code "${__ct_status}"
    for cmd in "${__ct_original_prompt_command[@]}"; do eval "${cmd:-}"; done
    __ct_precmd
  }
  __ct_prompt_cmd() { __ct_status="$?"; __ct_precmd; }
  __ct_original_prompt_command=${PROMPT_COMMAND:-}
  if [[ -n "${__ct_original_prompt_command:-}" && "${__ct_original_prompt_command:-}" != "__ct_prompt_cmd" ]]; then
    PROMPT_COMMAND=__ct_prompt_cmd_original
  else
    PROMPT_COMMAND=__ct_prompt_cmd
  fi
fi
"####
}

/// fish OSC 633 conf: wraps fish_prompt for A/B and uses preexec/postexec/prompt
/// events for E/C/D and cwd. Composes with the user's fish_prompt.
const fn include_fish_conf() -> &'static str {
    r####"# CodeTerm shell integration — do not edit
status is-interactive; or exit
set -q CODETERM_SHELL_INTEGRATION; and exit
set --global CODETERM_SHELL_INTEGRATION 1

if set -q CODETERM_SHELL_NONCE
  set --global __ct_nonce $CODETERM_SHELL_NONCE
  set -e CODETERM_SHELL_NONCE
end

function __ct_esc
  builtin printf "\e]633;%s\a" (string join ";" -- $argv)
end

function __ct_escape_value
  echo $argv | string replace --all '\\' '\\\\' | string replace --all ';' '\\x3b'
end

function __ct_cmd_executed --on-event fish_preexec
  __ct_esc E (__ct_escape_value "$argv") $__ct_nonce
  __ct_esc C
end

function __ct_cmd_finished --on-event fish_postexec
  __ct_esc D $status
end

function __ct_update_cwd --on-event fish_prompt
  __ct_esc P Cwd=(__ct_escape_value "$PWD")
  builtin printf '\e]7;file://%s%s\e\\' (hostname) "$PWD"
end

function __ct_fish_prompt_start
  __ct_esc A
end

function __ct_fish_cmd_start
  __ct_esc B
end

function __preserve_ct_fish_prompt --on-event fish_prompt
  if functions --query fish_prompt
    if functions --query __ct_fish_prompt
      functions --erase __ct_fish_prompt
    end
    functions --copy fish_prompt __ct_fish_prompt
    functions --erase __preserve_ct_fish_prompt
    __init_ct_shell_integration
  else
    functions --erase __preserve_ct_fish_prompt
    function __ct_fish_prompt
      echo -n (whoami)@(prompt_hostname) (prompt_pwd) '> '
    end
    __init_ct_shell_integration
  end
end

function __init_ct_shell_integration
  function fish_prompt
    __ct_fish_prompt_start
    __ct_fish_prompt
    __ct_fish_cmd_start
  end
end

__ct_esc P HasRichCommandDetection=True
__preserve_ct_fish_prompt
"####
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The portable-pty invariant behind the prod crash: a default-prog (login)
    /// builder panics the instant an arg is added.
    #[test]
    #[should_panic(expected = "default_prog")]
    fn default_prog_builder_panics_on_arg() {
        let mut cmd = CommandBuilder::new_default_prog();
        cmd.arg("--rcfile");
    }

    /// Regression guard. The plain terminal is built with `new_default_prog()`
    /// (a login shell). Before the fix the bash branch called
    /// `cmd.arg("--rcfile")` on it, panicking `spawn_pty` and leaving bash
    /// terminals blank — zsh used env only, so it was spared (the "x64 vs arm"
    /// report was really bash vs zsh). `apply_env` must never add args to a
    /// default-prog builder.
    #[test]
    fn apply_env_never_adds_args_to_default_prog_bash() {
        ensure_scripts(); // make the bashrc integration file exist — the panic precondition
        let mut cmd = CommandBuilder::new_default_prog();
        apply_env(&mut cmd, "/bin/bash"); // must not panic
        assert!(
            cmd.is_default_prog(),
            "apply_env must not add args to the default login-shell builder"
        );
    }

    /// An explicit-prog builder (specific shell/command) safely takes --rcfile.
    #[test]
    fn apply_env_keeps_explicit_bash_prog_spawnable() {
        ensure_scripts();
        let mut cmd = CommandBuilder::new("/bin/bash");
        apply_env(&mut cmd, "/bin/bash"); // must not panic
        assert!(!cmd.is_default_prog());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn apply_env_wires_powershell_command_finished_observation() {
        ensure_scripts();
        let script = integration_dir().join("powershell.ps1");
        let content = std::fs::read_to_string(&script).unwrap();
        let authenticated_command = content.find("']633;E;;'").unwrap();
        let command_finished = content.find("']633;D'").unwrap();
        assert!(content.contains("$env:CODETERM_SHELL_NONCE"));
        assert!(content.contains("Remove-Item Env:CODETERM_SHELL_NONCE"));
        assert!(content.contains("IsNullOrEmpty($global:__CodeTermShellNonce)"));
        assert!(authenticated_command < command_finished);

        let mut cmd = CommandBuilder::new("powershell.exe");
        apply_env(&mut cmd, "powershell.exe");
        let argv = cmd.get_argv();
        assert_eq!(argv[1], "-NoExit");
        assert_eq!(argv[2], "-File");
        assert_eq!(argv[3], script.as_os_str());
    }
}
