#!/usr/bin/env bash
# sched.sh: run the zcode job queue with at most 4 seats at once (2 on the ProArt), launches >= 3 min apart.
# A job that ends with a rate-limit/turn failure is re-queued after a 10-min cooldown. Exits (to wake the reviewer)
# when a job ends with EXIT 0, printing DONE <machine> <name>. State: queue.txt (pending), running.txt (launched).
S=${LM_OPS:-$HOME/lm-ops}
Q=$S/queue.txt; R=$S/running.txt; touch $R
out_of() { if [ "$1" = thor ]; then ssh -o ConnectTimeout=5 thor-lan "tail -c 3000 ~/lm-wt/$2.out 2>/dev/null"; else tail -c 3000 $S/wt/$2.out 2>/dev/null; fi; }
last=0
while true; do
  # reap finished
  while read -r m n mo t; do
    [ -z "$n" ] && continue
    o=$(out_of $m $n); code=$(echo "$o" | grep -oE '^EXIT [0-9]+' | tail -1 | awk '{print $2}')
    [ -z "$code" ] && continue
    sed -i "/^$m $n /d" $R
    if [ "$code" = 0 ]; then echo "DONE $m $n"; exit 0; fi
    if echo "$o" | grep -qE '429|1302|Turn execution failed|rate'; then
      echo "$m $n $mo $(( $(date +%s) + 600 ))" >> $Q.cool; echo "requeue $m $n (rate limit)" >&2
    else echo "FAILED $m $n (exit $code)"; exit 0; fi
  done < <(cat $R)
  # move cooled jobs back
  if [ -f $Q.cool ]; then now=$(date +%s); while read -r m n mo until; do [ "$now" -ge "$until" ] && echo "$m $n $mo" >> $Q || echo "$m $n $mo $until" >> $Q.cool2; done < $Q.cool; mv -f $Q.cool2 $Q.cool 2>/dev/null || rm -f $Q.cool; fi
  # launch next if room (main GLM-5.3 <= 4, flash <= 3, proart <= 2)
  main=$(grep -c ' main ' $R); flash=$(grep -c ' flash ' $R); pro=$(grep -c '^proart ' $R); now=$(date +%s)
  if [ $((now - last)) -ge 120 ] && [ -s $Q ]; then
    while read -r m n mo; do
      [ -z "$n" ] && continue; mo=${mo:-main}
      if [ "$mo" = main ] && [ "$main" -ge 4 ]; then continue; fi
      if [ "$mo" = flash ] && [ "$flash" -ge 3 ]; then continue; fi
      if [ "$m" = proart ] && [ "$pro" -ge 2 ]; then continue; fi
      [ "$m" = proart ] && [ "$(free -g | awk 'NR==2{print $7}')" -lt 4 ] && continue
      sed -i "0,/^$m $n\b/{/^$m $n\b/d}" $Q
      if [ "$m" = thor ]; then ssh thor-lan "sed -i '/^EXIT/d' ~/lm-wt/$n.out"; else sed -i '/^EXIT/d' $S/wt/$n.out; fi
      tmux has-session -t build 2>/dev/null || tmux new-session -d -s build -n idle
      $S/relaunch-z.sh $m $n $mo >&2 && echo "$m $n $mo $now" >> $R; last=$now; break
    done < <(cat $Q)
  fi
  [ ! -s $Q ] && [ ! -s $R ] && [ ! -s $Q.cool ] && { echo "QUEUE EMPTY"; exit 0; }
  sleep 60
done
