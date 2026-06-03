#!/bin/bash
################################################################################################
# this script launch robot framework tests on a target node
# Environment variables to set or modify:
#       NODE:                   target node for executing robot framework test suites
#       ALUMET_VERSION:         alumet version to test
#       ALUMET_DISTRIBUTION:    alumet distribution to test
#
################################################################################################

#set -x

# credentials used to logon on the target node
NODE=otpaas2
USERNAME=emmanuel
KEY=${HOME}/.ssh/id_rsa
HOME_TEST=$(pwd)

# version of Alumet to installed
ALUMET_VERSION=0.9.4
ALUMET_DISTRIBUTION=1_amd64_ubuntu_22.04


# Before executed the tests, we need to activate robot framework with the following command
# we deactivate the CI check on file $HOME/venv-robot/bin/activate
# shellcheck disable=SC1091
source "$HOME"/venv-robot/bin/activate

# you can exclude some tests using option --exclude following by TAG name 
# Available TAGs are : CPU, GPU, LOW, HIGH

echo "Start running tests at: $(date)"

robot   -v "NODE:localhost" \
        -v "USERNAME:$USERNAME" \
        -v "KEY:$KEY" \
        -v "NODE:$NODE"       \
        -v "HOME_TEST:$HOME_TEST"  \
        -v "ALUMET_VERSION:$ALUMET_VERSION" \
        -v "ALUMET_DISTRIBUTION:$ALUMET_DISTRIBUTION" \
        --metadata "Test are executed on node $NODE" \
        scenarios/

echo "End running tests at: $(date)"

# other tags defined on tests that can be exclude
        # --exclude RAPL_PLUGIN \
        # --exclude INSTALLATION \