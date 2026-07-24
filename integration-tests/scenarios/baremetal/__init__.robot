*** Settings ***
Documentation       This file initializes the test suite for Alumet integration tests.
...                 It sets up common resources and variables used across the test suite.

Resource            ../resources/alumet_keywords.resource

Suite Setup         Install Alumet
Suite Teardown      UnInstall Alumet


*** Variables ***
${FAKE_VARIABLE}    unused


*** Keywords ***
Install Alumet
    [Documentation]    Connect to a target node and install alumet
    ...     input parameters:
    ...       None
    ...     Return parameters:
    ...          stdout    of the command executed
    ...     Note that to connect to login, the following variables must be set as global:
    ...         ${NODE}                 : Node where alumet is installed
    ...         ${USERNAME}             : username to open the ssh connection
    ...         ${KEY}                  : ssh key to open the ssh connection
    ...         ${ALUMET_VERSION}       : alumet version
    ...         ${ALUMET_DISTRIBUTION}  : alumet distribution

    Log    fake variable: ${FAKE_VARIABLE}

    # first download the right linux package file, exit test suite if download error
    ${output}=    Run
    ...    wget https://github.com/alumet-dev/alumet/releases/download/v${ALUMET_VERSION}/alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb
    Log    output download package: ${output}
    ${exists}=    Run Keyword And Return Status
    ...    OperatingSystem.File Should Exist
    ...    alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb
    IF    not ${exists}
        Fail    'Error downloading alumet package file. Test suite is stopped'
    END

    Open Connection    ${GATEWAY}    alias=jumphost
    Login With Public Key    ${USERNAME}    ${KEY}

    Open Connection    ${NODE}

    Login With Public Key    ${USERNAME}    ${KEY}
    ...    jumphost_index_or_alias=jumphost

    # copy linux package on remote host
    Put File
    ...    alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb
    ...    alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb

    # copy tools files
    Put File    scenarios/tools/cpu_load.sh    cpu_load.sh

    VAR    ${command}=    sudo DEBIAN_FRONTEND=noninteractive apt install -y
    ...    ./alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb
    # install alumet package
    ${output}=    Execute Command Target Node    ${command}
    Log    result: ${output}

    # check if installation ok
    ${result}    ${stderr}=    Execute Command Target Node    apt list --installed alumet-agent
    Log    result: ${result}
    Log    stderr: ${stderr}

    # cancel test suite if installation failed
    ${exists}=    Run Keyword And Return Status    Should Contain    ${result}    alumet
    IF    not ${exists}
        Fail    'Error installing alumet. Test suite is stopped'
    END

    ${exists}=    Run Keyword And Return Status    Should Contain    ${result}    ${ALUMET_VERSION}
    IF    not ${exists}
        Fail    'Error installing alumet. Test suite is stopped'
    END

    Close All Connections

    RETURN

UnInstall Alumet
    [Documentation]    Connect to a target node and uninstall alumet
    ...     input parameters:
    ...         None
    ...     Return parameters:
    ...         stdout    of the command executed
    ...     Note that to connect to login, the following variables must be set as global:
    ...         ${NODE}                     : Node where alumet is installed
    ...         ${USERNAME}                 : username to open the ssh connection
    ...         ${KEY}                      : ssh key to open the ssh connection
    ...         ${ALUMET_VERSION}           : alumet version
    ...         ${ALUMET_DISTRIBUTION}      : alumet distribution
    ${output}    ${stderr}=    Execute Command Target Node
    ...    sudo DEBIAN_FRONTEND=noninteractive apt purge -y alumet-agent
    Log    output: ${output}
    Log    stderr: ${stderr}

    ${result}    ${stderr}=    Execute Command Target Node
    ...    apt list --installed alumet-agent
    Log    stderr: ${stderr}

    Should Not Contain    ${result}    alumet

    # remove alumet-output.csv file
    ${result}    ${stderr}=    Execute Command Target Node    rm alumet-output.csv
    Log    result: ${result}
    Log    stderr: ${stderr}

    # remove alumet package file on target node
    ${result}    ${stderr}=    Execute Command Target Node
    ...    rm alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb*
    Log    result: ${result}
    Log    stderr: ${stderr}

    ${result}    ${stderr}=    Execute Command Target Node
    ...    ls -l alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb*
    Log    result: ${result}
    Log    stderr: ${stderr}

    Should Not Contain    ${result}    alumet

    # remove alumet package file downloaded locally
    ${result}=    Run    rm alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb*
    Log    result: ${result}

    ${result}=    Run    ls -l alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb*
    Log    result: ${result}

    Should Contain    ${result}    cannot access

    # remove cpu_load.*
    ${result}    ${stderr}=    Execute Command Target Node    rm cpu_load.*
    Log    result: ${result}
    Log    stderr: ${stderr}

    ${result}    ${stderr}=    Execute Command Target Node    ls -l cpu_load.*
    Log    result: ${result}
    Log    stderr: ${stderr}

    Should Contain    ${stderr}    cannot access

    RETURN
