#!/bin/bash

source scripts-configuration.txt

if [ $# -lt 1 ]; then
  echo "Usage: bash $0 HOSTNAME"
  exit 1
fi

HOSTNAME=$1

check_success() {
    if [ $? -ne 0 ]; then
        echo "Error: $1 failed"
        exit 1
    fi
}

setup_node() {
    local hostname=$1
    echo "Setting up node $hostname..."

    ssh root@${hostname} "sed -i '/bullseye-backports/d' /etc/apt/sources.list"

    ssh root@${hostname} "apt update && apt install -y \
        libpowercap0 libpowercap-dev powercap-utils \
        libgflags-dev cgroup-tools \
        llvm-dev libclang-dev clang fio \
        parted python3-venv python3-pip python3-setuptools libusb-1.0-0"

    ssh root@${hostname} "wget -O - https://apt.corretto.aws/corretto.key | gpg --dearmor -o /usr/share/keyrings/corretto-keyring.gpg"
    ssh root@${hostname} "echo 'deb [signed-by=/usr/share/keyrings/corretto-keyring.gpg] https://apt.corretto.aws stable main' > /etc/apt/sources.list.d/corretto.list"
    ssh root@${hostname} "apt-get update && apt-get install -y java-1.8.0-amazon-corretto-jdk"

    # Setup Python virtual env
    ssh root@${hostname} "
        python3 -m venv /root/venv-quarchpy && \
        /root/venv-quarchpy/bin/pip install --upgrade pip && \
        /root/venv-quarchpy/bin/pip install --upgrade quarchpy
    "

    #ssh root@${hostname} "/root/venv-quarchpy/bin/python -m quarchpy.run fix_perm"

    # Mount of the disk
    ssh root@${hostname} "
        if [ -b /dev/nvme1n1 ]; then
            if [ ! -b /dev/nvme1n1p1 ]; then
                echo 'Creating partition on /dev/nvme1n1...'
                parted -s /dev/nvme1n1 mklabel gpt
                parted -s /dev/nvme1n1 mkpart primary ext4 0% 100%
                sleep 2
            fi
            if ! blkid /dev/nvme1n1p1 >/dev/null 2>&1; then
                echo 'Formatting /dev/nvme1n1p1...'
                mkfs.ext4 /dev/nvme1n1p1
            fi

			mkdir -p /tmp/nvme
			mount -o discard /dev/nvme1n1p1 /tmp/nvme
            if [ \$? -ne 0 ]; then
                echo '[WARN] Mount failed. Falling back to /tmp/nvme-fallback'
				mkdir -p /tmp/nvme-fallback
				ln -sfn /tmp/nvme-fallback /tmp/nvme
            fi
            chmod o+rwx /tmp/nvme
        else
            echo '[WARN] /dev/nvme1n1 not found. Using /tmp/nvme-fallback'
            mkdir -p /tmp/nvme-fallback
            ln -s /tmp/nvme-fallback /tmp/nvme
        fi
    "

    ssh root@${hostname} "grep -qxF '172.17.30.102   qtl2312-01-122' /etc/hosts || echo '172.17.30.102   qtl2312-01-122' | sudo tee -a /etc/hosts"

	#scp python_for_rust.py root@${hostname}:/python_for_rust.py
    scp alumet-agent_0.1.2-1_amd64.deb root@${hostname}:/usr/local/bin/alumet-agent.deb
    check_success "SCP script files"
    ssh root@${hostname} "cd /usr/local/bin; sudo apt install ./alumet-agent.deb"
    ssh root@${hostname} "sudo sysctl -w kernel.perf_event_paranoid=-1"
	ssh root@${hostname} "chmod +x /root/venv-quarchpy/lib/python3.11/site-packages/quarchpy/connection_specific/jdk_jres/lin_amd64_jdk_jre/bin/java"
}


echo "Starting node reservation process..."
JOB_SUBMISSION_OUTPUT=$(oarsub -t deploy -t exotic -p "$HOSTNAME" -l "host=1,walltime=2" "sleep 7200")

JOB_ID=$(echo "$JOB_SUBMISSION_OUTPUT" | grep -oP 'OAR_JOB_ID=\K\d+')
if [ -z "$JOB_ID" ]; then
    echo "Failed to extract JOB_ID."
    exit 1
fi
echo "Node reserved with job ID: $JOB_ID"

echo "Waiting for the job to start..."
while true; do
    sleep 5
    NODE_STATUS=$(oarstat -j "$JOB_ID" -f 2>/dev/null | grep -oP 'state = \K\w+')
    echo "Current job status: $NODE_STATUS"
    if [ "$NODE_STATUS" == "Running" ]; then
        break
    fi
done

echo "Job is running on node: $HOSTNAME"
echo "Deploying environment on $HOSTNAME with kadeploy..."
echo "$HOSTNAME" > node_list.txt
kadeploy3 -f node_list.txt -e debian12-nfs -k
check_success "kadeploy"
rm node_list.txt

setup_node "$HOSTNAME"
