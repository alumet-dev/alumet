*** Settings ***
Documentation   Alumet installation / uninstallation 
Library    OperatingSystem
Library    SSHLibrary
Library    String
Resource    ${HOME_TEST}/scenarios/common/alumet_keywords.robot

Suite Setup    Log     Test are running on cluster: ${NODE}  level=INFO

Test timeout    180 seconds

*** Keywords ***

*** Variables ***

# variables related to JOB submission
# ${Command}=        ${HOME_TEST}/tools/cpu_load.sh 10

*** Test Cases ***
Test connection on target node
    [Tags]    INSTALLATION

    ${output}    ${stderr}=    Execute Command Target Node    hostname
    Log    Output Result of SSH : ${output}

*** Test Cases ***
install alumet
    [Tags]    INSTALLATION

    # Alumet is already installed by Suite Setup
    # we check only if installed version is right

    ${result}    ${stderr}=    Execute Command Target Node    apt list --installed alumet-agent
    Log    Result stdout : ${result}

    Should Contain    ${result}    alumet
    should Contain    ${result}    ${ALUMET_VERSION}

*** Test Cases ***
which alumet-agent
    [Tags]    INSTALLATION

    ${output}    ${stderr}=    Execute Command Target Node    which alumet-agent
    Log    Result stdout : ${output}

    Should Contain    ${output}    /usr/bin/alumet-agent 

*** Test Cases ***
help option
    [Tags]    INSTALLATION

    ${file_content}=    OperatingSystem.Get File    scenarios/resources/help-option.txt

    ${output}    ${stderr}=    Execute Command Target Node    alumet-agent -h
    Log    Result stdout : ${output}

    Should Be Equal As Strings    ${file_content}    ${output}

*** Test Cases ***
help exec option
    [Tags]    INSTALLATION

    ${file_content}=    OperatingSystem.Get File    scenarios/resources/help-exec-option.txt

    ${output}    ${stderr}=    Execute Command Target Node    alumet-agent exec -h
    Log    Result stdout : ${output}

    Should Be Equal As Strings    ${file_content}    ${output}

*** Test Cases ***
help plugins option
    [Tags]    INSTALLATION

    ${file_content}=    OperatingSystem.Get File    scenarios/resources/help-plugins-option.txt

    ${output}    ${stderr}=    Execute Command Target Node    alumet-agent plugins -h
    Log    Result stdout : ${output}

    Should Be Equal As Strings    ${file_content}    ${output}    

*** Test Cases ***
help watch option
    [Tags]    INSTALLATION

    ${file_content}=    OperatingSystem.Get File    scenarios/resources/help-watch-option.txt

    ${output}    ${stderr}=    Execute Command Target Node    alumet-agent watch -h
    Log    Result stdout : ${output}

    Should Be Equal As Strings    ${file_content}    ${output}        

*** Test Cases ***
help config option
    [Tags]    INSTALLATION

    ${file_content}=    OperatingSystem.Get File    scenarios/resources/help-config-option.txt

    ${output}    ${stderr}=    Execute Command Target Node    alumet-agent config -h
    Log    Result stdout : ${output}

    Should Be Equal As Strings    ${file_content}    ${output}

*** Test Cases ***
config regen
    [Tags]    INSTALLATION

    ${output}    ${stderr}=    Execute Command Target Node    alumet-agent config regen
    Log    Result stdout : ${output}

    Should Contain     ${output}    Default configuration file written to: /etc/alumet/alumet-config.toml    




