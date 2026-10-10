#!/bin/zsh
set -eu
mode=${1:-output}
esc=$'\e'
case $mode in
  output)
    for n in {1..180}; do
      printf 'B3:%03d | λ 界 é | %s\n' $n ${(l:120::=:)${:-}}
    done
    printf '%s[31mRED%s[0m %s[32mGREEN%s[0m %s[7mINVERSE%s[0m\n' "$esc" "$esc" "$esc" "$esc" "$esc" "$esc"
    printf 'B3:%s\n' COMPLETE
    ;;
  alternate)
    trap 'printf "\e[?25h\e[?1049l"' EXIT
    trap 'printf "\e[?25h\e[?1049l"; trap - EXIT; exit 130' INT
    printf '%s[?1049h%s[2J%s[H%s[?25lB3:ALTERNATE\n' "$esc" "$esc" "$esc" "$esc"
    printf '%s[4;8H%s[36mB3:CURSOR-4-8%s[0m%s[?25h' "$esc" "$esc" "$esc" "$esc"
    read -r 'reply?Type an ASCII/Unicode token, or Ctrl+C: '
    printf '\nB3:ECHO:%s\n' "$reply"
    ;;
  *) exit 2;;
esac
