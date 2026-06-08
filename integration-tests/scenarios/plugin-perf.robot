*** Settings ***
Documentation   Alumet test plugin perf 
Library    OperatingSystem
Library    SSHLibrary
Library    String
Resource    ${HOME_TEST}/scenarios/common/alumet_keywords.robot


Test timeout    60 seconds

*** Keywords ***

*** Variables ***

# variables related to JOB submission
# ${Command}=        ${HOME_TEST}/tools/cpu_load.sh 10

*** Test Cases ***
Test connection on target node
    [Tags]    INPUT_PLUGIN     PERF_PLUGIN  

    ${output}    ${stderr}=    Execute Command Target Node    hostname
    Log    Output Result of SSH : ${output}

*** Test Cases ***
run cpu_load
    [Tags]    INPUT_PLUGIN     PERF_PLUGIN  

    ${output}    ${stderr}=    Execute Command Target Node   nohup ./cpu_load.sh 20 > /dev/null 2>&1 &
    Sleep    3s

*** Test Cases ***
run plugin csv perf
    [Tags]    INPUT_PLUGIN     PERF_PLUGIN  

    ${output}    ${stderr}=    Execute Command Target Node    alumet-agent --plugins csv,perf watch "$(cat cpu_load.sh.pid)" > /tmp/alumet.log 2>&1 &
    Sleep    3s

    ${output_alumet}    ${stderr}=    Execute Command Target Node    cat /tmp/alumet.log
    Log    Result stdout : ${output_alumet}

    # Should Contain     ${output_alumet}    ${expected_started_plugins}    
    Should Contain     ${output_alumet}    2 plugins started
    Should Contain     ${output_alumet}    csv v0.2.0
    Should Contain     ${output_alumet}    perf v0.1.0
    
*** Test Cases ***
check alumet running
    [Tags]    INPUT_PLUGIN     PERF_PLUGIN  

    ${output}    ${stderr}=    Execute Command Target Node    ps -f -u ${USERNAME}
    Log    Result stdout : ${output}

    Should Contain     ${output}    /usr/lib/alumet-agent --plugins csv,perf

*** Test Cases ***
Check Perf Metric perf_hardware_REF_CPU_CYCLES 
    [Template]    Check Metric    
    [Tags]    INPUT_PLUGIN     PERF_PLUGIN    
    
        # ${metric}                ${resource_kind}    ${domain}
    perf_hardware_REF_CPU_CYCLES        local_machine   

Check Perf Metric perf_hardware_CACHE_MISSES 
    [Template]    Check Metric    
    [Tags]    INPUT_PLUGIN     PERF_PLUGIN    
    
        # ${metric}                ${resource_kind}    ${domain}
    perf_hardware_CACHE_MISSES        local_machine  
    
Check Perf Metric perf_hardware_BRANCH_MISSES 
    [Template]    Check Metric    
    [Tags]    INPUT_PLUGIN     PERF_PLUGIN    
    
        # ${metric}                ${resource_kind}    ${domain}
    perf_hardware_BRANCH_MISSES        local_machine  


*** Test Cases ***
Check alumet not running
    [Tags]    INPUT_PLUGIN     PERF_PLUGIN  

    ${output}    ${stderr}=    Execute Command Target Node    ps -f -u ${USERNAME}

    Should Not Contain     ${output}    alumet-agent


