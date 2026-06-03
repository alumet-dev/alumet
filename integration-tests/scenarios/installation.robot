*** Settings ***
Documentation   Alumet installation / uninstallation 
Library    OperatingSystem
Library    SSHLibrary
Library    String
Resource    ${HOME_TEST}/scenarios/common/alumet_keywords.robot

Suite Setup    Log     Test are running on cluster: ${NODE}  level=INFO

Test timeout    180 seconds

*** Keywords ***
Display current date
    ${date}=    Get Time    result_format=%Y-%m-%d %H:%M:%S
    Log To Console    Current date : ${date}

*** Variables ***

# variables related to JOB submission
# ${Command}=        ${HOME_TEST}/tools/cpu_load.sh 10

*** Test Cases ***
Test connection on target node
    [Tags]    INSTALLATION

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
    [Tags]    INSTALLATION

    ${output}=   Install Alumet
    Log    Result stdout : ${output}

    ${result}=    Execute Command Alumet Node    apt list --installed alumet-agent
    Log    Result stdout : ${result}

    Should Contain    ${result}    alumet
    should Contain    ${result}    ${ALUMET_VERSION}

*** Test Cases ***
which alumet-agent
    [Tags]    INSTALLATION

    ${output}=    Execute Command Alumet Node    which alumet-agent
    Log    Result stdout : ${output}

    Should Contain    ${output}    /usr/bin/alumet-agent 

*** Test Cases ***
help option
    [Tags]    INSTALLATION

    ${file_content}=    OperatingSystem.Get File    scenarios/resources/help-option.txt

    ${output}=    Execute Command Alumet Node    alumet-agent -h
    Log    Result stdout : ${output}

    # Should Contain    ${output}    Usage
    # Should Contain    ${output}    Commands    
    # Should Contain    ${output}    Options    

    Should Be Equal As Strings    ${file_content}    ${output}

*** Test Cases ***
help exec option
    [Tags]    INSTALLATION

    ${file_content}=    OperatingSystem.Get File    scenarios/resources/help-exec-option.txt

    ${output}=    Execute Command Alumet Node    alumet-agent exec -h
    Log    Result stdout : ${output}

    Should Be Equal As Strings    ${file_content}    ${output}

*** Test Cases ***
help plugins option
    [Tags]    INSTALLATION

    ${file_content}=    OperatingSystem.Get File    scenarios/resources/help-plugins-option.txt

    ${output}=    Execute Command Alumet Node    alumet-agent plugins -h
    Log    Result stdout : ${output}

    Should Be Equal As Strings    ${file_content}    ${output}    

*** Test Cases ***
help watch option
    [Tags]    INSTALLATION

    ${file_content}=    OperatingSystem.Get File    scenarios/resources/help-watch-option.txt

    ${output}=    Execute Command Alumet Node    alumet-agent watch -h
    Log    Result stdout : ${output}

    Should Be Equal As Strings    ${file_content}    ${output}        

*** Test Cases ***
help config option
    [Tags]    INSTALLATION

    ${file_content}=    OperatingSystem.Get File    scenarios/resources/help-config-option.txt

    ${output}=    Execute Command Alumet Node    alumet-agent config -h
    Log    Result stdout : ${output}

    Should Be Equal As Strings    ${file_content}    ${output}

*** Test Cases ***
config regen
    [Tags]    INSTALLATION

    ${output}=    Execute Command Alumet Node    alumet-agent config regen
    Log    Result stdout : ${output}

    Should Contain     ${output}    Default configuration file written to: /etc/alumet/alumet-config.toml

*** Test Cases ***
uninstall alumet
    [Tags]    INSTALLATION
  
    UnInstall Alumet
    




