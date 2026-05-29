*** Settings ***
Documentation   Alumet test plugin rapl 
Library    OperatingSystem
Library    SSHLibrary
Library    String
Resource    ${HOME_TEST}/scenarios/common/alumet_keywords.robot

Suite Setup        Install Alumet
Suite Teardown     UnInstall Alumet

Test timeout    60 seconds

*** Keywords ***


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
run plugins socket-control csv rapl
    [Tags]

    ${output}=    Execute Command Alumet Node    alumet-agent --plugins csv,rapl,socket-control > /tmp/alumet.log 2>&1 &
    Sleep    3s

    ${output_alumet}=    Execute Command Alumet Node    cat /tmp/alumet.log
    # ${output}=    Execute Command Alumet Node    date; ls -l
    Log    Result stdout : ${output_alumet}

    # Should Contain     ${output_alumet}    ${expected_started_plugins}    
    Should Contain     ${output_alumet}    3 plugins started
    Should Contain     ${output_alumet}    csv v0.2.0
    Should Contain     ${output_alumet}    socket-control v0.2.1
    Should Contain     ${output_alumet}    rapl v0.3.1
    
*** Test Cases ***
check alumet running
    [Tags]    INPUT_PLUGIN     RAPL_PLUGIN

    ${output}=    Execute Command Alumet Node    ls alumet-control.sock
    Log    Result stdout : ${output}

    Should Contain     ${output}    alumet-control.sock

    ${output}=    Execute Command Alumet Node    ps -f -u ${USERNAME}
    Log    Result stdout : ${output}

    Should Contain     ${output}    /usr/lib/alumet-agent --plugins csv,rapl,socket-control

*** Test Cases ***
check metric rapl_consumed_energy_J resource cpu_package domain package
    [Tags]     INPUT_PLUGIN     RAPL_PLUGIN

    # read the csv output file,  resource_kind domain package
    ${output}=     Read resource_kind    rapl_consumed_energy_J    package
    
    Should Contain     ${output}    cpu_package

    # read the csv output file,  metric value
    ${output}=    Read value        rapl_consumed_energy_J    cpu_package    package

    Should Be True    ${output} !=0.0

*** Test Cases ***
check metric rapl_consumed_energy_J resource cpu_package domain package_total
    [Tags]     INPUT_PLUGIN     RAPL_PLUGIN

    # read the csv output file,  resource_kind domain package
    ${output}=     Read resource_kind    rapl_consumed_energy_J    package_total
    
    Should Contain     ${output}    local_machine

    # read the csv output file,  metric value
    ${output}=    Read value        rapl_consumed_energy_J    local_machine    package_total

    Should Be True    ${output} !=0.0

*** Test Cases ***
check metric rapl_consumed_energy_J resource dram domain dram
    [Tags]     INPUT_PLUGIN     RAPL_PLUGIN

    # read the csv output file,  resource_kind domain dram
    ${output}=     Read resource_kind    rapl_consumed_energy_J    dram
    
    Should Contain     ${output}    dram

    # read the csv output file,  metric value
    ${output}=    Read value        rapl_consumed_energy_J    dram    dram

    Should Be True    ${output} !=0.0

*** Test Cases ***
check metric rapl_consumed_energy_J resource dram domain dram_total
    [Tags]     INPUT_PLUGIN     RAPL_PLUGIN

    # read the csv output file,  resource_kind domain dram
    ${output}=     Read resource_kind    rapl_consumed_energy_J    dram_total
    
    Should Contain     ${output}    local_machine

    # read the csv output file,  metric value
    ${output}=    Read value        rapl_consumed_energy_J    local_machine    dram_total
    
    Should Be True    ${output} !=0.0

*** Test Cases ***
stop alumet
    [Tags]

    ${output}=    Execute Command Alumet Node    echo "shutdown" | socat UNIX-CONNECT:alumet-control.sock -    
    Log    Result stdout : ${output}  
    # ${output}=    Execute Command Alumet Node    sudo apt remove -y --purge alumet-agent/now   
    # Log    Result stdout : ${output}

    ${output}=    Execute Command Alumet Node    ls alumet-control.sock

    Should Not Contain     ${output}    alumet-control.sock
   
*** Test Cases ***
Check alumet not running
    [Tags]

    ${output}=    Execute Command Alumet Node    echo "shutdown" | socat UNIX-CONNECT:alumet-control.sock -    
    Log    Result stdout : ${output}  
    # ${output}=    Execute Command Alumet Node    sudo apt remove -y --purge alumet-agent/now   
    # Log    Result stdout : ${output}

    ${output}=    Execute Command Alumet Node    ls alumet-control.sock

    Should Not Contain     ${output}    alumet-control.sock

    ${output}=    Execute Command Alumet Node    ps -f -u ${USERNAME}

    Should Not Contain     ${output}    alumet-agent


