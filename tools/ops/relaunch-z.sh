#!/usr/bin/env bash
# relaunch-z.sh thor|proart <name>: append the CONTINUE note to the job's BRIEF.md and launch zcode on it in the right tmux.
M=$1; N=$2; MODEL=${3:-main}; if [ "$MODEL" = flash ]; then MF="--model account:zai-individual-coding-plan/GLM-5.3-Flash"; else MF=""; fi; S=${LM_OPS:-$HOME/lm-ops}
if [ "$M" = thor ]; then
  scp -q $S/cont-note.md thor-lan:/tmp/cont-note.md
  ssh thor-lan "cat /tmp/cont-note.md >> ~/lm-wt/$N/BRIEF.md; tmux new-window -t last-mile -n z-$N \"cd ~/lm-wt/$N && source ~/.cargo/env; export PATH=\\\$HOME/.nvm/versions/node/v24.22.2/bin:\\\$PATH; zcode -p 'Read BRIEF.md in this directory, especially the CONTINUE section at the end, and carry it out exactly. Only modify files inside this directory. Answer in English.' --cwd ~/lm-wt/$N $MF > ~/lm-wt/$N.out 2>&1; echo EXIT \\\$? >> ~/lm-wt/$N.out\""
else
  W=/home/proart/lm-wt/$N; cat $S/cont-note.md >> $W/BRIEF.md
  tmux new-window -t build -n z-$N "cd $W && zcode -p 'Read BRIEF.md in this directory, especially the CONTINUE section at the end, and carry it out exactly. Only modify files inside this directory (plus screenshot files). Answer in English.' --cwd $W $MF > $S/wt/$N.out 2>&1; echo EXIT \$? >> $S/wt/$N.out"
fi
echo "relaunched $N on $M ($MODEL)"
