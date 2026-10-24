# git-custom-cli (`gct`)

`gct` is a keyboard-driven terminal UI for making Git commits with a custom date and time, a branch to push to, and a multiline message. You fill in the fields from a menu, review a summary, and then `gct` stages everything, creates the commit, and optionally pushes it to `origin`. It is written in Rust on top of `crossterm`, and it runs on Windows, macOS and Linux.

## Features

- Menu-driven commit flow with a "fields ready" counter (date, time, branch, message).
- Keyboard date (`YYYY-MM-DD`) and time (`HH:MM:SS`) masks that reject invalid values.
- Inline single-line and multiline message editors, plus an external editor option.
- Startup summary of the current branch and working-tree status.
- Normal commit, amend mode, and dry run (preview only).
- Commit date applied to both author and committer date.
- Optional push to `origin` after the commit, with `--force-with-lease` when amending.
- Last date, branch and message are remembered for the next run.
- A small mascot in the header that turns toward the mouse pointer.
- Saved Git accounts (label, name, email, GitHub username) that you can switch between. See [Accounts](#accounts).

## Requirements

- Rust and Cargo, to install or build from source. The crate uses edition 2021.
- Git available on `PATH`.
- An interactive terminal. `gct` refuses to run when stdin or stdout is not a terminal.
- Run it from inside a Git repository.

> [!IMPORTANT]
> `gct` must be started directly in a shell. Piping its output or running it from a script without a terminal makes it exit with `gct needs an interactive terminal; run it directly in a shell`.

## Installation

Install the published crate:

```sh
cargo install git-custom-cli
```

The package is named `git-custom-cli`, but the installed binary is named `gct`.

To build from source instead:

```sh
git clone <repository-url>
cd git-custom-cli
cargo build --release
```

The optimized binary is written to `target/release/gct` (`target\release\gct.exe` on Windows).

## Quick start

1. Open a terminal in a Git repository that has changes to commit.
2. Run `gct`.
3. Set the date, time, branch and message from the menu. Use `Enter` on each row.
4. Choose **Finalize & commit**.
5. Confirm the summary, then answer the push question.

To save several Git accounts and switch between them, run `gct --account`. See [Accounts](#accounts).

## Setup

### Git identity

The header shows the identity `gct` reads from `git config --get user.name` and `user.email` (local or global config) as `Name <email>`, or `not set` when neither is configured. It is read once at startup. Until you activate a saved account (see [Accounts](#accounts)), `gct` does not set or change this identity; the commit uses whatever Git uses. Once an account is active, its name and email are the identity used for commits, and its GitHub username is the login used for pushes (see [Accounts](#accounts)).

### First run

On the first run there is no saved configuration, so the date and time start as `[not set]`, the branch defaults to the branch you have checked out, and the message is empty. Fill in the four fields once, then commit. After a successful commit, those values become the defaults for the next run.

### Saved configuration

`gct` stores the last date, branch and message, along with your saved accounts, in a JSON file:

| OS | Path |
| --- | --- |
| Windows | `%USERPROFILE%\.config\git-commit-tui\config.json` |
| macOS / Linux | `~/.config/git-commit-tui/config.json` |

The commit values are written only after a successful commit (never for a dry run). They use three keys: `last_date`, `last_branch` and `last_message`. Saved accounts are written as soon as you add, edit, delete or activate one (see [Accounts](#accounts)).

> [!NOTE]
> The folder and file are created the first time `gct` saves after a successful commit.

To reset the saved values:

```sh
gct --clear-config
```

This deletes the file and prints `Saved configuration cleared.` It also deletes saved accounts. It does not undo a global Git account that is still active, so choose **git default** in the account screen first if you want the original global values back. It works even when the file is corrupt, and it does not need a Git repository or a terminal.

> [!TIP]
> If `gct` reports `invalid config file`, run `gct --clear-config` to delete the broken file.

## Accounts

`gct` can save several Git accounts and switch between them. An account has:

- a **label**, the name shown in `gct`,
- a **git name**, written to `user.name`,
- an **email**, written to `user.email`,
- a **GitHub username**, used for push login.

Saved accounts are stored in the config file (see [Saved configuration](#saved-configuration)). Two accounts cannot share an email, because GitHub attributes commits by email, so `gct` rejects a duplicate.

### Using accounts

Choose **Account** in the main menu to open the picker. Pick a saved account to make it active, or pick **git default** to stop using a `gct` account. The Account row shows the active account's label.

Activating an account sets these three global Git settings:

- `user.name`
- `user.email`
- `credential.https://github.com.username`

> [!WARNING]
> Activating an account changes **global** Git config. Every commit and push from every tool on this machine uses it (terminal, VS Code and others), not only `gct`. Before the first activation, `gct` records the global values it finds. Choosing **git default**, or deleting the active account, restores those recorded values. Switching between saved accounts only rewrites the three settings above.

> [!NOTE]
> A repo-local `user.name` or `user.email` overrides the global account in that repository. `gct` never changes repo-local settings, so commits in such a repository keep the local identity. To check which value applies, run `git config --get user.email` inside the repository.

**Amend** keeps the original author, as Git does by default. Only the committer changes to the active account.

**Commits and pushes** use the active account. A commit records the active account's name and email. A push authenticates as the active account's GitHub username, because Git Credential Manager picks the login from `credential.https://github.com.username`. `gct` does not pass these values on the command line, so a repo-local `user.name`, `user.email` or `credential.https://github.com.username` overrides the account in that repository. Check it with `git config --get user.email` and `git config --get credential.https://github.com.username` inside the repository.

**Push login**: Git Credential Manager keeps one GitHub login per host, so the first push after switching accounts may open a browser window to sign in as the new account. Apart from the one profile request described below, `gct` never reads or stores tokens.

> [!IMPORTANT]
> Push as the account that can write to the repository. If the repository is owned by a different GitHub account, or by an organization you belong to through another account, activate that account before you push.

> [!WARNING]
> A `403` from GitHub during a push means the active account cannot write to that repository. `gct` shows Git's error text and does not switch accounts for you. The commit stays local. Activate an account with write access, then push the local commit with `git push origin <branch>`. `gct` has no push-only mode, so running it again would create a new commit.

### Managing accounts (gct --account)

Run `gct --account` to open the account screen. The first row is **git default**, followed by the saved accounts. You can add, edit, delete and activate accounts from here.

| Key | Action |
| --- | --- |
| Up / Down, `w` / `s` (any case) | Move the selection |
| Enter | Set the selected account as active. On **git default**, stop using a `gct` account |
| `a` (any case) | Add an account. Enter a local label (for example `account 1`) and the GitHub username. `gct` opens the browser to sign in as that user, then fills in the name and email from your GitHub profile |
| `l` (any case) | Sign in again to the selected account, then refresh its name and email from GitHub |
| `r` (any case) | Refresh the selected account's name and email from GitHub, without signing in again |
| `e` (any case) | Edit the selected account's label and display name |
| `d` (any case) | Delete the selected account. `gct` asks for confirmation first |
| Esc / `q` (any case) | Leave the account screen |

The header shows the git identity that is active now (`Active in git: Name <email>`), read from your git config on every frame. Each account row shows its display name (or its email if the name is empty), its email, its GitHub username, `signed in` or `not signed in`, and `(active)` for the account git uses.

Email comes from your public GitHub email when there is one. Otherwise `gct` uses GitHub's no-reply address (`ID+username@users.noreply.github.com`), which still attributes commits to your account.

> [!WARNING]
> To read your profile, `gct` asks Git Credential Manager for the GitHub token and uses it for one request to `api.github.com`. The token is kept in memory only. `gct` never writes it to a file or prints it.

## Usage

### Main menu

The screen shows a header with the mascot, the branch and the working-tree status (first three `git status --short` lines, plus a count of the rest). This status is read once when `gct` starts and does not refresh.

The menu shows `MISSION CONTROL [n/4 FIELDS READY]` and these rows:

| Row | What it does |
| --- | --- |
| Edit date | Opens the date mask |
| Edit time | Opens the time mask |
| Edit branch | Sets the branch to push to |
| Edit message | Opens the inline message editor |
| Edit msg (editor) | Opens the message in an external editor |
| Edit options | Toggles amend and dry run (shows `normal`, `amend`, `dry-run` or `amend, dry-run`) |
| Account | Opens the account picker. The row shows the active account (see [Accounts](#accounts)) |
| View history | Opens the git history screen (see below) |
| Finalize & commit | Validates the fields, shows the summary, and runs the commit |

| Key | Action |
| --- | --- |
| Up / Down, `w` / `s` (any case) | Move the selection (the separator row is skipped) |
| Enter / Space | Open the selected row |
| Esc / `q` (any case) | Quit |
| Ctrl+C | Quit |

If you try to finalize with a missing or invalid field, the menu stays open and shows the problem in red: `Please set a valid date`, `Please set a valid time (HH:MM:SS)`, `Please choose a branch`, or `Please write a commit message`.

> [!NOTE]
> `gct` refuses to run when HEAD is detached. Check out a branch first, because a commit or push from a detached HEAD would not go to a branch.

### History

Choose **View history** in the menu to read the repository without leaving `gct`. The screen has two tabs:

- **Recent commits**: the last 200 commits, with the short hash, date, decorations (branches and tags) and subject.
- **Remote branches**: the branches under `git branch -r`, with `origin/HEAD` left out.

The top line shows the current branch and its upstream, or `no upstream`. In a repository with no commits, the screen shows an empty list.

| Key | Action |
| --- | --- |
| Tab | Switch between Recent commits and Remote branches |
| Up / Down, `w` / `s` (any case) | Scroll one line |
| PageUp / PageDown | Scroll one page |
| Home / End | Jump to the top or bottom |
| Esc / `q` (any case) | Return to the menu |
| Ctrl+C | Quit |

### Date and time masks

Each digit fills the highlighted slot and the cursor moves to the next one. Separators are fixed.

| Key | Action |
| --- | --- |
| `0`-`9` | Type a digit into the highlighted slot |
| Left / Right | Move between slots |
| Home / End | Jump to the first / last slot |
| Backspace | Clear the current slot and move left |
| Delete | Clear the current slot |
| Enter | Save (rejects incomplete or invalid values) |
| Esc | Cancel and keep the previous value |
| Ctrl+C | Quit `gct` |

Validation messages: `Complete all fields before continuing`, `Not a valid calendar date` (leap years are handled), and `Time out of range (00:00:00 - 23:59:59)`.

### Text editors

The branch field uses the single-line editor. The message uses the multiline editor.

| Key | Single line (branch) | Multiline (message) |
| --- | --- | --- |
| Typing, Left / Right, Home / End, Backspace, Delete | Edit text | Edit text |
| Enter | Save | New line |
| Ctrl+S | Not used | Save (Ctrl+Enter also saves where the terminal reports it) |
| Esc | Cancel and keep the previous value | Cancel and keep the previous value |
| Ctrl+C | Quit `gct` | Quit `gct` |

An empty value is rejected with `Value is required` (single line) or `Message is required` (multiline). Saved messages are trimmed.

### Confirmation prompts

| Prompt | Keys | Default on Enter |
| --- | --- | --- |
| Amend the last commit instead of creating a new one? | Y / N / Enter | No |
| Preview only (dry run, do not commit/push)? | Y / N / Enter | No |
| Proceed with these actions? (final summary) | Y / N / Enter | Yes |
| Push to GitHub? (after the commit) | Y / N / Enter | Yes |
| Press any key to exit (final screen) | Any key | - |

Ctrl+C at the summary prompt cancels without committing and quits. Ctrl+C at the push question does not undo the commit: it is treated as "no push", and `gct` shows `Done. commit -> local only (no push)`.

> [!WARNING]
> The **Edit branch** value only selects the push target. The commit is always created on the branch that is currently checked out. The push runs `git push origin <branch>` for the value you entered, so make sure the two match.

## Commit modes

Choose these under **Edit options**. Both reset to off each time `gct` starts.

### Normal commit

Runs, in order:

```text
git add .
git commit -m "<message>"
```

The date you entered is passed to Git as `GIT_AUTHOR_DATE` and `GIT_COMMITTER_DATE`. `git add .` stages from the directory where you started `gct`, and it stages all changes in that directory tree.

### Amend

Runs `git add .` and then `git commit --amend -m "<message>"`. The date fields are applied to the amended commit as well, and the message field is still required.

> [!CAUTION]
> When you push an amended commit, `gct` uses `git push --force-with-lease origin <branch>`. This rewrites the remote branch. The lease refuses the push if the remote has commits you have not fetched, but the previous commit on the remote is still replaced.

### Dry run

Dry run validates the fields and then prints the commands it would run, without staging, committing, saving the configuration or pushing:

```text
Dry run - commands that would be executed:
  git add .
  git commit -m "<message>" (author/committer date: <date>)
  git push origin <branch>
```

Dry run combines with amend, and the printout then shows `commit --amend`.

## External editor

Choose **Edit msg (editor)** to edit the message in an editor. `gct` writes the current message to a temporary file, leaves the TUI, runs the editor on that file, and reads it back when the editor exits. The message is trimmed, and an empty result is rejected.

The editor is chosen in this order:

1. `VISUAL`
2. `EDITOR`
3. `notepad` on Windows
4. `vi` on macOS and Linux

The variable can include arguments, which are split on spaces. The temporary file path is added last.

```powershell
$env:EDITOR = "code --wait"
gct
```

> [!NOTE]
> The editor must block until you close it (for example, `code --wait`). If it returns immediately, `gct` reads the file before you have written anything.

## Troubleshooting

| Message | Meaning and fix |
| --- | --- |
| `gct needs an interactive terminal; run it directly in a shell` | Stdin or stdout is not a terminal. Run `gct` from a shell window. |
| `Not a git repository` | The current branch or `git status` could not be read. Run `gct` inside a repository. |
| `could not start git <args>` | The `git` program could not be started. Check that Git is on `PATH`. |
| `git command failed` | A Git command failed without printing an error. Run the same command by hand to see why. |
| `Staging all files failed: <git message>` | `git add .` failed. The Git message follows the colon. |
| `<git message>` after `Error:` during commit or push | Git rejected the commit or push. The text is Git's own stderr, for example a rejected push. |
| `could not start git commit` | The `git commit` process could not be started. |
| `Please set a valid date` / `Please set a valid time (HH:MM:SS)` / `Please choose a branch` / `Please write a commit message` | A required field is missing or invalid. Fix it in the menu, then finalize again. |
| `Complete all fields before continuing` | A date or time mask still has empty slots. |
| `Not a valid calendar date` | The date does not exist, for example `2026-02-30`. |
| `Time out of range (00:00:00 - 23:59:59)` | The time is not a valid 24-hour time. |
| `Value is required` / `Message is required` | An editor was saved empty. |
| `editor command is empty` | `VISUAL` or `EDITOR` is set but contains no program name. |
| `editor exited with status <status>` | The editor closed with a non-zero status, so the change was discarded. |
| `could not read editor output` | The temporary file could not be read after the editor closed. |
| `Commit message is required` | The external editor returned an empty message. |
| `could not locate the home directory` | No home directory was found, so the config path cannot be built. |
| `could not read <path>` / `could not write <path>` / `could not create <path>` | A file-system error on the config file or its folder. Check permissions. |
| `invalid config file <path>` | The config file is not valid JSON for `gct`. Run `gct --clear-config`. |
| `could not enable terminal raw mode` / `could not enter terminal UI` | The terminal does not support the required mode. |
| `Cancelled.` (exit code 130) | You pressed Ctrl+C, or Esc / `q` in the main menu. Nothing was committed by this action. |

## FAQ

**Does `gct` push automatically?**
No. After the commit it asks `Push to GitHub?`. The default is Yes, so press `n` to keep the commit local.

**If the push fails, is the commit lost?**
No. The config is saved and the commit already exists locally. The push error is printed, and the commit remains.

**Does `gct` stage only some files?**
No. Normal and amend modes run `git add .`, which stages all changes under the directory you started `gct` in. Stage selectively with Git before running `gct` if you need that.

**Why does the mascot disappear?**
The mascot is drawn only when the terminal is at least 50 columns wide and 32 rows tall. Below that size the header shows only the title and tagline.

**What does the mascot do?**
It turns its eyes and nose toward your mouse pointer as you move it over the terminal, and its face and status line change with the state (ready, working, done, or error).

**Why can't I select text with the mouse?**
`gct` turns on mouse capture while it runs, so most terminals stop handling mouse selection. Check your terminal's documentation for a way to bypass this.

**Is the dry run saved for next time?**
No. A dry run never saves the configuration. The date, time, branch and message are remembered only after a real commit.

**Why use `--force-with-lease` for amend?**
It is a safer force push: it refuses to overwrite remote commits that your local copy has not seen.

## Development

Check formatting, lint, and build:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
```

Project layout:

```text
src/main.rs      Entry point, CLI flag, and the commit workflow
src/input.rs     Menu, text and date/time editors, external editor, confirmations
src/terminal.rs  Centered panel, drawing, mouse tracking, and terminal setup/restore
src/mascot.rs    Mascot frames, colors, and the size check
src/git.rs       Git subprocess calls (branch, status, add, commit, push)
src/config.rs    Saved configuration load, save and clear
src/accounts.rs  Saved accounts and applying the active one to global Git config
src/model.rs     Values, validation and text helpers
```

## License

Licensed under the MIT License. See [LICENSE](LICENSE).
