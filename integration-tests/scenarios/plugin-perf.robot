*** Settings ***
Documentation   Alumet test plugin perf 
Library    OperatingSystem
Library    SSHLibrary
Library    String
Resource    ${HOME_TEST}/scenarios/common/alumet_keywords.robot

Suite Setup        Install Alumet
Suite Teardown     UnInstall Alumet
# Suite Setup    Log     Test are running on cluster: ${NODE}  level=INFO

Test timeout    60 seconds

*** Keywords ***
Display current date
    ${date}=    Get Time    result_format=%Y-%m-%d %H:%M:%S
    Log To Console    Current date : ${date}

*** Variables ***

# variables related to JOB submission
# ${Command}=        ${HOME_TEST}/tools/cpu_load.sh 10

*** Test Cases ***
Test connection on target node
    [Tags]

    Open Connection     172.16.118.53    alias=jumphost
    Login With Public Key             ${USERNAME}     ${KEY}

    Open Connection    ${NODE}
    
    Login With Public Key    ${USERNAME}     ${KEY}
    ...    jumphost_index_or_alias=jumphost


    ${output}=    Execute Command    hostname
    Log    Output Result of SSH : ${output}

    Close All Connections

*** Test Cases ***
run cpu_load
    [Tags]    INPUT_PLUGIN     PERF_PLUGIN  

    ${output}=    Execute Command Alumet Node   nohup ./cpu_load.sh 20 > /dev/null 2>&1 &
    Sleep    3s

*** Test Cases ***
run plugin csv perf
    [Tags]    INPUT_PLUGIN     PERF_PLUGIN  

    ${output}=    Execute Command Alumet Node    alumet-agent --plugins csv,perf,socket-control watch "$(cat cpu_load.sh.pid)" > /tmp/alumet.log 2>&1 &
    Sleep    3s

    ${output_alumet}=    Execute Command Alumet Node    cat /tmp/alumet.log
    # ${output}=    Execute Command Alumet Node    date; ls -l
    Log    Result stdout : ${output_alumet}

    # Should Contain     ${output_alumet}    ${expected_started_plugins}    
    Should Contain     ${output_alumet}    3 plugins started
    Should Contain     ${output_alumet}    csv v0.2.0
    Should Contain     ${output_alumet}    socket-control v0.2.1
    Should Contain     ${output_alumet}    perf v0.1.0
    
*** Test Cases ***
check alumet running
    [Tags]    INPUT_PLUGIN     PERF_PLUGIN  

    ${output}=    Execute Command Alumet Node    ls alumet-control.sock
    Log    Result stdout : ${output}

    Should Contain     ${output}    alumet-control.sock

    ${output}=    Execute Command Alumet Node    ps -f -u ${USERNAME}
    Log    Result stdout : ${output}

    Should Contain     ${output}    /usr/lib/alumet-agent --plugins csv,perf,socket-control

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
stop alumet
    [Tags]    INPUT_PLUGIN     PERF_PLUGIN  

    ${output}=    Execute Command Alumet Node    echo "shutdown" | socat UNIX-CONNECT:alumet-control.sock -    
    Log    Result stdout : ${output}  
    
    ${output}=    Execute Command Alumet Node    ls alumet-control.sock

    Should Not Contain     ${output}    alumet-control.sock
   
*** Test Cases ***
Check alumet not running
    [Tags]    INPUT_PLUGIN     PERF_PLUGIN  

    ${output}=    Execute Command Alumet Node    echo "shutdown" | socat UNIX-CONNECT:alumet-control.sock -    
    Log    Result stdout : ${output}  
    # ${output}=    Execute Command Alumet Node    sudo apt remove -y --purge alumet-agent/now   
    # Log    Result stdout : ${output}

    ${output}=    Execute Command Alumet Node    ls alumet-control.sock

    Should Not Contain     ${output}    alumet-control.sock

    ${output}=    Execute Command Alumet Node    ps -f -u ${USERNAME}

    Should Not Contain     ${output}    alumet-agent


