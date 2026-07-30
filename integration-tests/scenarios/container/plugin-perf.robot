*** Settings ***
Documentation       Alumet test plugin perf

Library             OperatingSystem
Library             SSHLibrary
Library             Process
Library             String

Resource            ../resources/alumet_keywords.resource

Test Timeout        60 seconds

Test Tags           input_plugin    perf_plugin


*** Test Cases ***
Run cpu_load
    [Documentation]    Execute cpu_load script in the background

    Copy tools File
    
    VAR  ${command}=    nohup ./cpu_load.sh 20 > /dev/null 2>&1 &
    ${output}   ${stderr}=    Execute Command Target Node     ${command}
    Sleep    3s
    Log    Output Result of SSH : ${output}
    Log    stderr Result of SSH : ${stderr}

Run plugin csv perf
    [Documentation]    Run alumet-agent with csv and perf plugins

    Install Alumet As Container    csv,perf
    #watch "$(cat cpu_load.sh.pid)" 
    
    ${result}   ${stderr}=    Execute Command Target Node       sudo podman logs ${ALUMET_CONTAINER_NAME}
    Log    result: ${result}
    Log    stderr: ${stderr}

    Should Contain    ${stderr}    Starting Alumet
    Should Contain    ${stderr}    ${ALUMET_VERSION}

    Should Contain    ${stderr}    4 metrics registered

    # check that csv and perf plugins are started
    ${started_section}=    Get Regexp Matches
    ...    ${stderr}
    ...    plugins started:(.*?)plugins disabled:
    ...    1
    ...    flags=DOTALL
    Should Contain    ${started_section}[0]    csv
    Should Contain    ${started_section}[0]    perf

Check alumet running
    [Documentation]    Verify that alumet-agent is running with the correct plugins

    ${output}   ${stderr}=    Execute Command Target Node       sudo podman exec ${ALUMET_CONTAINER_NAME} ps -f
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    Should Contain    ${output}    /usr/bin/alumet-agent
    Should Contain    ${output}    --plugins csv,perf

Copy csv File
    [Documentation]    Copy alumet csv file

    # wait several seconds to get some metrics in csv file
    sleep    10s
    Copy csv File

Check Perf Metric perf_hardware_REF_CPU_CYCLES
    [Documentation]    Check perf_hardware_REF_CPU_CYCLES metric
    [Template]    Check Metric
    # ${metric}                   ${resource_kind}    ${domain}
    perf_hardware_REF_CPU_CYCLES    local_machine      ${EMPTY}

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

Check Perf Metric perf_cache_LL_READ_MISS
    [Documentation]    Check perf_cache_LL_READ_MISS metric
    [Template]    Check Metric

    # ${metric}    ${resource_kind}    ${domain}
    perf_cache_LL_READ_MISS    local_machine

Stop alumet
    [Documentation]    Stop alumet-agent delete alumet container

    UnInstall Alumet As Container
    Log    Stop alumet

Check alumet not running
    [Documentation]    Verify that alumet-agent is not running

    ${output}    ${stderr}=    Execute Command Target Node    ps -f -u ${USERNAME}
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    Should Not Contain    ${output}    alumet-agent
