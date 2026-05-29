*** Settings ***
Documentation   Alumet test plugin perf 
Library    OperatingSystem
Library    SSHLibrary
Library    String
Resource    ${HOME_TEST}/scenarios/common/alumet_keywords.robot

Suite Setup    Log     Test are running on cluster: ${NODE}  level=INFO

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
install alumet
    [Tags]

    ${output}=    Execute Command Alumet Node    sudo DEBIAN_FRONTEND=noninteractive apt install -y ./alumet-agent_${ALUMET_VERSION}_${ALUMET_DISTRIBUTION}.deb
    Log    Result stdout : ${output}

    ${result}=    Execute Command Alumet Node    apt list --installed alumet-agent
    Log    Result stdout : ${result}

    Should Contain    ${result}    alumet
    should Contain    ${result}    ${ALUMET_VERSION}


*** Test Cases ***
run plugin csv perf
    [Tags]

    ${output}=    Execute Command Alumet Node    alumet-agent --plugins csv,perf,socket-control > /tmp/alumet.log 2>&1 &
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
    [Tags]

    ${output}=    Execute Command Alumet Node    ls alumet-control.sock
    Log    Result stdout : ${output}

    Should Contain     ${output}    alumet-control.sock

    ${output}=    Execute Command Alumet Node    ps -f -u ${USERNAME}
    Log    Result stdout : ${output}

    Should Contain     ${output}    /usr/lib/alumet-agent --plugins csv,perf,socket-control

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


