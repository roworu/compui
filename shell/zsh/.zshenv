# found through ZDOTDIR, set by compui-pty. the user's ZDOTDIR is put back first, so their own
# files are read as usual
if [[ -n "$COMPUI_ZDOTDIR" ]]; then ZDOTDIR="$COMPUI_ZDOTDIR"; else unset ZDOTDIR; fi
unset COMPUI_ZDOTDIR
[[ -f "${ZDOTDIR:-$HOME}/.zshenv" ]] && source "${ZDOTDIR:-$HOME}/.zshenv"
source "${COMPUI_SHELL_DIR}/compui.zsh"
# ghostty only closes without asking at a prompt its shell integration marked, and it injects that
# only into the shell it starts itself, not into this one
if [[ -o interactive && -r "$GHOSTTY_RESOURCES_DIR/shell-integration/zsh/ghostty-integration" ]]; then
  builtin autoload -Uz -- "$GHOSTTY_RESOURCES_DIR/shell-integration/zsh/ghostty-integration"
  ghostty-integration
  builtin unfunction ghostty-integration
fi
