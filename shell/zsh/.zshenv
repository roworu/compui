# found through ZDOTDIR, set by compui-pty. the user's ZDOTDIR is put back first, so their own
# files are read as usual
if [[ -n "$COMPUI_ZDOTDIR" ]]; then ZDOTDIR="$COMPUI_ZDOTDIR"; else unset ZDOTDIR; fi
unset COMPUI_ZDOTDIR
[[ -f "${ZDOTDIR:-$HOME}/.zshenv" ]] && source "${ZDOTDIR:-$HOME}/.zshenv"
source "${COMPUI_SHELL_DIR}/compui.zsh"
