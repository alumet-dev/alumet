*** Settings ***
Documentation       Alumet test plugin perf

Library             OperatingSystem
Library             SSHLibrary
Resource            ../resources/alumet_keywords.resource

Test Timeout        60 seconds

Test Tags           input_plugin    perf_plugin


*** Test Cases ***
Test connection on target node
    [Documentation]    Verify SSH connection to the target node

    ${output}    ${stderr}=    Execute Command Target Node    hostname
    Log    Output Result of SSH : ${output}
    Log    stderr Result of SSH : ${stderr}

Run cpu_load
    [Documentation]    Execute cpu_load script in the background

    ${output}    ${stderr}=    Execute Command Target Node    nohup ./cpu_load.sh 20 > /dev/null 2>&1 &
    Sleep    3s
    Log    Output Result of SSH : ${output}
    Log    stderr Result of SSH : ${stderr}

Run plugin csv perf
    [Documentation]    Run alumet-agent with csv and perf plugins

    ${output}    ${stderr}=    Execute Command Target Node
    ...    alumet-agent --plugins csv,perf watch "$(cat cpu_load.sh.pid)" > /tmp/alumet.log 2>&1 &
    Sleep    3s
    Log    Output Result of SSH : ${output}
    Log    stderr Result of SSH : ${stderr}

    ${output_alumet}    ${stderr}=    Execute Command Target Node    cat /tmp/alumet.log
    Log    Result stdout : ${output_alumet}
    Log    stderr Result : ${stderr}

    Should Contain    ${output_alumet}    2 plugins started
    Should Contain    ${output_alumet}    csv v0.2.0
    Should Contain    ${output_alumet}    perf v0.1.0

Check alumet running
    [Documentation]    Verify that alumet-agent is running with the correct plugins

    ${output}    ${stderr}=    Execute Command Target Node    ps -f -u ${USERNAME}
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    Should Contain    ${output}    /usr/lib/alumet-agent --plugins csv,perf

Check Perf Metric perf_hardware_REF_CPU_CYCLES
    [Documentation]    Check perf_hardware_REF_CPU_CYCLES metric
    [Template]    Check Metric
    # ${metric}    ${resource_kind}    ${domain}
    perf_hardware_REF_CPU_CYCLES    local_machine

Check Perf Metric perf_hardware_CACHE_MISSES
    [Documentation]    Check perf_hardware_CACHE_MISSES metric
    [Template]    Check Metric

    # ${metric}    ${resource_kind}    ${domain}
    perf_hardware_CACHE_MISSES    local_machine

Check Perf Metric perf_hardware_BRANCH_MISSES
    [Documentation]    Check perf_hardware_BRANCH_MISSES metric
    [Template]    Check Metric

    # ${metric}    ${resource_kind}    ${domain}
    perf_hardware_BRANCH_MISSES    local_machine

Check alumet not running
    [Documentation]    Verify that alumet-agent is not running

    ${output}    ${stderr}=    Execute Command Target Node    ps -f -u ${USERNAME}
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    Should Not Contain    ${output}    alumet-agent
