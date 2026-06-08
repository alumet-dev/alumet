*** Settings ***
Documentation   Alumet test plugin rapl 
Library    OperatingSystem
Library    SSHLibrary
Library    String
Resource    ${HOME_TEST}/scenarios/common/alumet_keywords.robot
# Test Template    Check Metric    
Test timeout    60 seconds

*** Keywords ***


*** Variables ***

# variables related to JOB submission
# ${Command}=        ${HOME_TEST}/tools/cpu_load.sh 10

*** Test Cases ***
Test connection on target node
    [Tags]    INPUT_PLUGIN     RAPL_PLUGIN    

    ${output}    ${stderr}=    Execute Command Target Node    hostname
    Log    Output Result of SSH : ${output}

*** Test Cases ***
run plugins socket-control csv rapl
    [Tags]    INPUT_PLUGIN     RAPL_PLUGIN

    ${output}    ${stderr}=    Execute Command Target Node    alumet-agent --plugins csv,rapl,socket-control > /tmp/alumet.log 2>&1 &
    Sleep    3s

    ${output_alumet}    ${stderr}=    Execute Command Target Node    cat /tmp/alumet.log
    Log    Result stdout : ${output_alumet}

    # Should Contain     ${output_alumet}    ${expected_started_plugins}    
    Should Contain     ${output_alumet}    3 plugins started
    Should Contain     ${output_alumet}    csv v0.2.0
    Should Contain     ${output_alumet}    socket-control v0.2.1
    Should Contain     ${output_alumet}    rapl v0.3.1
    
*** Test Cases ***
check alumet running
    [Tags]    INPUT_PLUGIN     RAPL_PLUGIN

    ${output}    ${stderr}=    Execute Command Target Node    ls alumet-control.sock
    Log    Result stdout : ${output}

    Should Contain     ${output}    alumet-control.sock

    ${output}    ${stderr}=    Execute Command Target Node    ps -f -u ${USERNAME}
    Log    Result stdout : ${output}

    Should Contain     ${output}    /usr/lib/alumet-agent --plugins csv,rapl,socket-control

    ${result}=  Compare values percent      100     105     8

    Should Be True     ${result}

*** Test Cases ***
Check Rapl Metric package
    [Template]    Check Metric    
    [Tags]    INPUT_PLUGIN     RAPL_PLUGIN    
    # ${metric}                ${resource_kind}    ${domain}
    rapl_consumed_energy_J        cpu_package        package    

Check Rapl Metric package_total
    [Template]    Check Metric    
    [Tags]    INPUT_PLUGIN     RAPL_PLUGIN    
    # ${metric}                ${resource_kind}    ${domain}
    rapl_consumed_energy_J    local_machine        package_total    

Check Rapl Metric dram
    [Template]    Check Metric    
    [Tags]    INPUT_PLUGIN     RAPL_PLUGIN    
    # ${metric}                ${resource_kind}    ${domain}
    rapl_consumed_energy_J        dram                dram    

Check Rapl Metric dram_total
    [Template]    Check Metric    
    [Tags]    INPUT_PLUGIN     RAPL_PLUGIN    
    # ${metric}                ${resource_kind}    ${domain}
    rapl_consumed_energy_J        local_machine    dram_total    

*** Test Cases ***
stop alumet
    [Tags]    INPUT_PLUGIN     RAPL_PLUGIN

    ${output}    ${stderr}=    Execute Command Target Node    echo "shutdown" | socat UNIX-CONNECT:alumet-control.sock -    
    Log    Result stdout : ${output}  

    ${output}    ${stderr}=    Execute Command Target Node    ls alumet-control.sock

    Should Not Contain     ${output}    alumet-control.sock
   
*** Test Cases ***
Check alumet not running
    [Tags]    INPUT_PLUGIN     RAPL_PLUGIN

    ${output}=    Execute Command Target Node    echo "shutdown" | socat UNIX-CONNECT:alumet-control.sock -    
    Log    Result stdout : ${output}  

    ${output}    ${stderr}=    Execute Command Target Node    ls alumet-control.sock

    Should Not Contain     ${output}    alumet-control.sock

    ${output}    ${stderr}=    Execute Command Target Node    ps -f -u ${USERNAME}

    Should Not Contain     ${output}    alumet-agent