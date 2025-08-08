#!/bin/bash

source scripts-configuration.txt

if [ $# -lt 3 ]; then
  echo "Usage: bash $0 HOSTNAME [PLUGIN1,PLUGIN2,...] COMMAND_TO_EXEC [ARGS...]"
  exit 1
fi

HOSTNAME="$1"
shift

IFS=',' read -ra PLUGINS <<< "$1"
shift

# By default, add always "quarch" at the beginning
PLUGIN_LIST="quarch"
for p in "${PLUGINS[@]}"; do
  [[ "$p" != "quarch" ]] && PLUGIN_LIST+=",${p}"
done

COMMAND_TO_EXEC="$*"

EXPERIMENT_START_TIME=$(date +%Y-%m-%d-%H-%M-%S)
EXPERIMENT_DIRECTORY="${EXPERIMENT_RESULTS_DIRECTORY}/${EXPERIMENT_START_TIME}"

echo "Do you want to keep the current config for the result directory?"
echo "-----"
echo ${EXPERIMENT_DIRECTORY}
echo "-----"
read -p "Use this config? [Y/n] " CONFIRM
if [[ "$CONFIRM" =~ ^[Nn]$ ]]; then
    read -p "Enter the path you want to use for the results: " CUSTOM_DIRECTORY
    EXPERIMENT_DIRECTORY="${CUSTOM_DIRECTORY}"
fi

mkdir -p "$EXPERIMENT_DIRECTORY"
ssh root@${HOSTNAME} "mkdir -p $EXPERIMENT_DIRECTORY && echo 'Directory created successfully'"

ssh root@${HOSTNAME} "alumet-agent config regen"
echo "Do you want to keep the current config for alumet?"
echo "-----"
ssh root@${HOSTNAME} "cat ${DEFAULT_CONFIG}"
echo "-----"
read -p "Use this config? [Y/n] " CONFIRM
if [[ "$CONFIRM" =~ ^[Nn]$ ]]; then
    read -p "Enter the path to the config file to use: " CUSTOM_CONFIG
    CONFIG_ARG="--config ${CUSTOM_CONFIG}"
else
    CONFIG_ARG="" 
fi

OUTPUT_FILE="${EXPERIMENT_DIRECTORY}/alumet-output.csv"
echo "Do you want to keep the current output file name?"
echo "-----"
echo "$OUTPUT_FILE"
echo "-----"
read -p "Use this output file name? [Y/n] " CONFIRM
if [[ "$CONFIRM" =~ ^[Nn]$ ]]; then
    read -p "Enter the new output file name: " CUSTOM_OUTPUT_FILE
    OUTPUT_FILE="${EXPERIMENT_DIRECTORY}/${CUSTOM_OUTPUT_FILE}"
fi

# Debug statements
echo "PLUGIN_LIST: $PLUGIN_LIST"
echo "COMMAND_TO_EXEC: $COMMAND_TO_EXEC"

ssh root@${HOSTNAME} "source /root/venv-quarchpy/bin/activate && alumet-agent ${CONFIG_ARG} --output-file "${OUTPUT_FILE}" --plugins \"${PLUGIN_LIST}\" exec ${COMMAND_TO_EXEC}"
#ssh root@${HOSTNAME} "cd ~ && python3 python_for_rust.py > ${EXPERIMENT_DIRECTORY}/power.log"

echo -e "\n\n Gathering experiment results..."
scp -r root@${HOSTNAME}:${EXPERIMENT_DIRECTORY}/* ${EXPERIMENT_DIRECTORY}/
echo -e " Done.\n"
