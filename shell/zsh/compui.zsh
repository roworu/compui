# compUI zsh adapter, ported from kuterm's completion adapter: the line editor reports its line to
# compui-pty over $COMPUI_SOCKET and accepted completions are put into the line here
if [[ -o interactive ]]; then
  autoload -Uz add-zsh-hook
  # autocomplete: the line editor reports its line to compui over the tab's private socket,
  # and compui sends picked completions back, which are put into the line here. without the
  # socket module or zle hooks (zsh before 5.3) it stays off and nothing else changes
  if [[ -n "$COMPUI_SOCKET" ]] && zmodload zsh/net/socket 2>/dev/null \
    && autoload -Uz add-zle-hook-widget 2>/dev/null; then
    typeset -gi __compui_cfd=-1 __compui_cseq=0
    typeset -g __compui_csent= __compui_cpath= __compui_cpwd= __compui_cpwdjson=
    typeset -g __compui_hstamp=
    typeset -ga __compui_hlines
    zmodload zsh/parameter 2>/dev/null

    # each function starts with "emulate -L zsh": user options like ksh_arrays or
    # sh_word_split must not change how they run

    # $1 as a json string in REPLY. other control characters become one char each, so the
    # cursor still counts the same chars as the line
    __compui_cjson() {
      emulate -L zsh
      local s=$1
      # most lines need no escaping, one match is cheaper than the five passes
      if [[ $s != *[\\\"[:cntrl:]]* ]]; then
        REPLY="\"$s\""
        return
      fi
      s=${s//\\/\\\\}
      s=${s//\"/\\\"}
      s=${s//$'\n'/\\n}
      s=${s//$'\t'/\\t}
      s=${s//[[:cntrl:]]/\\ufffd}
      REPLY="\"$s\""
    }

    __compui_cclose() {
      emulate -L zsh
      zle -F $__compui_cfd 2>/dev/null
      exec {__compui_cfd}>&- 2>/dev/null
      __compui_cfd=-1
    }

    __compui_csend() {
      emulate -L zsh
      (( __compui_cfd >= 0 )) || return
      # a closed socket must not kill the shell with SIGPIPE, the trap is back on return so
      # programs started later get the usual one
      setopt localtraps
      trap '' PIPE
      print -r -- "$1" >&$__compui_cfd 2>/dev/null || __compui_cclose
    }

    # a picked completion: "1 accept <seq> <start> <end> <text>", the text runs to the end.
    # compui is told whether it was put in, so it can do it again on a line that changed
    __compui_creceive() {
      emulate -L zsh
      local line
      if ! IFS= read -r -u $1 line; then
        # compui went away
        __compui_cclose
        return
      fi
      local -a f
      f=("${(@s: :)line}")
      [[ $f[1] == 1 && $f[2] == accept && $f[3] == <-> ]] || return
      local seq=$f[3] start=$f[4] end=$f[5] text=${line#* * * * * }
      # keys typed since the last report may have changed the line, which then gets a new seq
      __compui_creport
      # an answer to an older line would edit the wrong text
      if [[ $seq != $__compui_cseq || $start != <-> || $end != <-> ]] \
        || (( start > end || end > $#BUFFER )); then
        __compui_csend "{\"v\":1,\"type\":\"answered\",\"seq\":$seq,\"applied\":false}"
        return
      fi
      BUFFER="${BUFFER[1,start]}$text${BUFFER[end+1,-1]}"
      CURSOR=$(( start + $#text ))
      # a redraw from here runs no line-pre-redraw hook, so they are run here: they report
      # the new line, and plugins like zsh-syntax-highlighting color it there. the hook
      # dispatcher finds its hooks by $WIDGET, which still names this handler
      if (( $+functions[azhw:zle-line-pre-redraw] )); then
        () {
          local -h -r WIDGET=zle-line-pre-redraw
          azhw:zle-line-pre-redraw
        }
      else
        __compui_creport
      fi
      __compui_csend "{\"v\":1,\"type\":\"answered\",\"seq\":$seq,\"applied\":true}"
      zle -R
    }
    zle -N __compui_creceive

    # recent history lines, newest first, rebuilt only when the history changes so a key
    # scans an array; ${#history} is linear on the special parameter, so HISTCMD is the stamp
    __compui_hrefresh() {
      emulate -L zsh
      [[ ${HISTCMD:-0} == $__compui_hstamp ]] && return
      __compui_hstamp=${HISTCMD:-0}
      local -a lines
      local key=$(( ${HISTCMD:-0} - 1 )) scans=0
      while (( key > 0 && scans < 1000 && $#lines < 500 )); do
        (( ${+history[$key]} )) && lines+=("${history[$key]}")
        (( scans++, key-- ))
      done
      __compui_hlines=("${lines[@]}")
    }

    # up to 3 history lines starting with the text before the cursor, as ',"recent":[...]' in REPLY
    __compui_crecent() {
      emulate -L zsh
      local typed=${BUFFER[1,CURSOR]} entry out=
      local -a seen
      REPLY=
      [[ -n ${typed//[[:space:]]/} ]] || return
      __compui_hrefresh
      for entry in "${(@M)__compui_hlines:#$typed*}"; do
        (( $#seen >= 3 )) && break
        [[ $entry == "$BUFFER" || $entry == *[[:cntrl:]]* ]] && continue
        (( ${seen[(Ie)$entry]} )) && continue
        seen+=("$entry")
        __compui_cjson "$entry"
        out+=${out:+,}$REPLY
      done
      REPLY=${out:+,\"recent\":[$out]}
    }

    __compui_creport() {
      emulate -L zsh
      if (( __compui_cfd < 0 )); then
        zsocket "$COMPUI_SOCKET" 2>/dev/null || return
        __compui_cfd=$REPLY
        zle -F -w $__compui_cfd __compui_creceive
        __compui_cpath=
        __compui_csend '{"v":1,"type":"hello","shell":"zsh"}'
      fi
      if [[ "$PATH" != "$__compui_cpath" ]]; then
        __compui_cpath=$PATH
        __compui_cjson "$PATH"
        __compui_csend "{\"v\":1,\"type\":\"env\",\"path\":$REPLY}"
      fi
      # redraws without a change send nothing
      [[ "$CURSOR:$BUFFER" == "$__compui_csent" ]] && return
      __compui_csent="$CURSOR:$BUFFER"
      (( ++__compui_cseq ))
      local buffer hist=
      # moving through history must not pop completions over the recalled line, the next
      # up or down has to scroll the history again
      [[ $LASTWIDGET == *history* || $LASTWIDGET == *-line-or-search ]] && hist=,"history":true
      __compui_cjson "$BUFFER"
      buffer=$REPLY
      if [[ $PWD != "$__compui_cpwd" ]]; then
        __compui_cpwd=$PWD
        __compui_cjson "$PWD"
        __compui_cpwdjson=$REPLY
      fi
      __compui_crecent
      __compui_csend "{\"v\":1,\"type\":\"line\",\"seq\":$__compui_cseq,\"cursor\":$CURSOR,\"buffer\":$buffer,\"cwd\":$__compui_cpwdjson$hist$REPLY}"
    }

    __compui_cfinish() {
      emulate -L zsh
      __compui_csent=
      __compui_csend '{"v":1,"type":"end"}'
    }

    # line-init hooks added this early never run, so they go in before the first prompt
    __compui_chooks() {
      add-zsh-hook -d precmd __compui_chooks
      add-zle-hook-widget line-init __compui_creport
      # runs on every key: a plain widget skips add-zle-hook-widget's dispatcher (~0.1ms a key).
      # one added later still runs it, add-zle-hook-widget keeps a widget it finds there
      if (( $+widgets[zle-line-pre-redraw] )); then
        add-zle-hook-widget line-pre-redraw __compui_creport
      else
        zle -N zle-line-pre-redraw __compui_creport
      fi
      add-zle-hook-widget line-finish __compui_cfinish
    }
    add-zsh-hook precmd __compui_chooks
  fi
fi
