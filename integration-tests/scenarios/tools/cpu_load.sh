#!/bin/bash
################################################################################################
# This script executes a very simple loop for cpu load.
# It calculate the Fibonacci suite: sigma 0 to N during the input period in seconds.
#
# It takes 1 input argument:
#       1. duration jobs in seconds
################################################################################################
export HOST=$(hostname)
date
echo "PID $0: $$"
echo "$$" > $0.pid
echo "running load script on host: $HOST"

if [ $# -lt 1 ]
   then
        echo "Parameters required :"
        echo "Missing input parameter: duration of load script in seconds"
     exit 2
fi


N=$1

i=0
result=0

start=$(date +%s)        # start timestamp (seconds since Epoch)

while (( $(date +%s) - start < N )); do
    # ---- below the cpu load code ----
    i=$((i+1))
    result=$(($result+i))
    echo -e "$(date +%T): \u03A3 $i = $result"
    tput cuu1
    # -------------------------
done

echo -e "\n \u03A3 $i = $result"
echo -e "\n $(date)"