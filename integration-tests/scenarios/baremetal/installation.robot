*** Settings ***
Documentation       Alumet installation / uninstallation

Library             OperatingSystem
Library             SSHLibrary
Resource            ../resources/alumet_keywords.resource

Suite Setup         Log    Test are running on cluster: ${NODE}    level=INFO
Test Timeout        180 seconds

Test Tags           installation


*** Test Cases ***
Test connection on target node
    [Documentation]    Verify SSH connection to the target node

    ${output}    ${stderr}=    Execute Command Target Node    hostname
    Log    Output Result of SSH : ${output}
    Log    stderr Result of SSH : ${stderr}

Install alumet
    [Documentation]    Check if alumet is installed with the correct version

    # Alumet is already installed by Suite Setup
    # we check only if installed version is right

    ${result}    ${stderr}=    Execute Command Target Node    apt list --installed alumet-agent
    Log    Result stdout : ${result}
    Log    stderr Result : ${stderr}

    Should Contain    ${result}    alumet
    Should Contain    ${result}    ${ALUMET_VERSION}

Which alumet-agent
    [Documentation]    Verify the location of alumet-agent binary

    ${output}    ${stderr}=    Execute Command Target Node    which alumet-agent
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    Should Contain    ${output}    /usr/bin/alumet-agent

Help option
    [Documentation]    Test the help option

    ${file_content}=    OperatingSystem.Get File    scenarios/resources/help-option.txt

    ${output}    ${stderr}=    Execute Command Target Node    alumet-agent -h
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    Should Be Equal As Strings    ${file_content}    ${output}

Help exec option
    [Documentation]    Test the help exec option

    ${file_content}=    OperatingSystem.Get File    scenarios/resources/help-exec-option.txt

    ${output}    ${stderr}=    Execute Command Target Node    alumet-agent exec -h
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    Should Be Equal As Strings    ${file_content}    ${output}

Help plugins option
    [Documentation]    Test the help plugins option

    ${file_content}=    OperatingSystem.Get File    scenarios/resources/help-plugins-option.txt

    ${output}    ${stderr}=    Execute Command Target Node    alumet-agent plugins -h
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    Should Be Equal As Strings    ${file_content}    ${output}

Help watch option
    [Documentation]    Test the help watch option

    ${file_content}=    OperatingSystem.Get File    scenarios/resources/help-watch-option.txt

    ${output}    ${stderr}=    Execute Command Target Node    alumet-agent watch -h
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    Should Be Equal As Strings    ${file_content}    ${output}

Help config option
    [Documentation]    Test the help config option

    ${file_content}=    OperatingSystem.Get File    scenarios/resources/help-config-option.txt

    ${output}    ${stderr}=    Execute Command Target Node    alumet-agent config -h
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    Should Be Equal As Strings    ${file_content}    ${output}

Config regen
    [Documentation]    Test the config regen option

    ${output}    ${stderr}=    Execute Command Target Node    alumet-agent config regen
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    Should Contain    ${output}    Default configuration file written to: /etc/alumet/alumet-config.toml
