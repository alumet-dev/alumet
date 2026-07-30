*** Settings ***
Documentation       This resource file contains reusable keywords for interacting with the Alumet monitoring system.
...                 It includes keywords for:
...                 - Comparing values with percentage precision
...                 - Executing commands on remote nodes via SSH
...                 - Installing and uninstalling the Alumet agent
...                 - Reading and checking metrics from Alumet's output CSV file

Library             OperatingSystem
Library             SSHLibrary

*** Variables ***
${ALUMET_CONTAINER_NAME}    alumet_robot_fm


*** Keywords ***
Compare Values Percent
    [Documentation]    Compare 2 values with a precision expressed as a %
    ...    input parameters:
    ...         value1
    ...         value2
    ...    precision in %
    ...    return parameter:
    ...         boolean: true if value1 = value2 according the precision
    [Arguments]    ${value1}    ${value2}    ${percent}

    ${diff}=    Evaluate    abs(${value1}-${value2})/${value1}*100
    ${result}=    Evaluate    ${diff} < ${percent}

    RETURN    ${result}

Execute Command Target Node
    [Documentation]    Connect to a target node and execute a command on this node
    ...     input parameters:
    ...         Command:     the command to execute on remote host
    ...     Return parameters:
    ...         stdout    of the command executed
    ...     Note that to connect to login, the following variables must be set as global:
    ...     ${NODE}         : Node where alumet is installed
    ...     ${USERNAME}     : username to open the ssh connection
    ...     ${KEY}          : ssh key to open the ssh connection
    [Arguments]    ${Command}

    Open Connection    ${GATEWAY}    alias=jumphost
    Login With Public Key    ${USERNAME}    ${KEY}

    Open Connection    ${NODE}

    Login With Public Key    ${USERNAME}    ${KEY}
    ...    jumphost_index_or_alias=jumphost

    ${stdout}    ${stderr}    ${rc}=    Execute Command    ${Command}
    ...    timeout=30s
    ...    return_stdout=True
    ...    return_stderr=True
    ...    return_rc=True
    Log    Result stdout : ${stdout}
    Log    Result stderr : ${stderr}
    Log    Result return code : ${rc}

    Close All Connections

    RETURN    ${stdout}    ${stderr}

Read Resource Kind
    [Documentation]    Connect to a target node and read the column  resource_kind of alumet-output.csv file.
    ...     input parameters:
    ...         metric:    metric name to parse
    ...         domain: domain name
    ...     Return parameters:
    ...         stdout    of the command executed
    ...     Note that to connect to login, the following variables must be set as global:
    ...         ${NODE}                     : Node where alumet is installed
    ...         ${USERNAME}                 : username to open the ssh connection
    ...         ${KEY}                      : ssh key to open the ssh connection
    [Arguments]    ${metric}    ${domain}=${EMPTY}

    # the metric resource_kind is on 4th column
    IF    '${domain}' != ''
        VAR    ${command}=    grep ${metric} alumet-output.csv | awk -F ';' ' $8 == "${domain}" { OFS="|"; print $4 }'
    ELSE
        VAR    ${command}=    grep ${metric} alumet-output.csv
    END

    ${output}    ${stderr}=    Execute Command Target Node    ${command}
    Log    output: ${output}
    Log    stderr: ${stderr}

    RETURN    ${output}

Read Value
    [Documentation]    Connect to a target node and read the metric value on the first line of alumet-output.csv file
    ...     input parameters:
    ...         metric:    metric name to parse
    ...         resource_kind: resource kind name
    ...         domain: domain name
    ...     Return parameters:
    ...         stdout    metric value
    ...     Note that to connect to login, the following variables must be set as global:
    ...         ${NODE}                     : Node where alumet is installed
    ...         ${USERNAME}                 : username to open the ssh connection
    ...         ${KEY}                      : ssh key to open the ssh connection
    [Arguments]    ${metric}    ${resource_kind}    ${domain}=${EMPTY}

    # sed -n 'Np' command below means that we read the N first metric value found in the csv file
    # it is better not to read the first value but second or higher
    # the metric value is on 3rd column (print $3)
    IF    '${domain}' != ''
        VAR    ${command}=
        ...    grep ${metric} alumet-output.csv | awk -F ';' ' $8 == "${domain}" &&
        ...    $4 == "${resource_kind}" { OFS="|"; print $3 }' | sed -n '2p'
    ELSE
        VAR    ${command}=
        ...    grep ${metric} alumet-output.csv | awk -F ';' '
        ...    $4 == "${resource_kind}" { OFS="|"; print $3 }' | sed -n '2p'
    END

    ${output}    ${stderr}=    Execute Command Target Node    ${command}
    Log    output: ${output}
    Log    stderr: ${stderr}

    Log To Console    metric value read: ${output}

    RETURN    ${output}

Check Metric
    [Documentation]    Connect to a target node and check metric using the 2 keywords: Read Resource Kind and Read Value
    ...     input parameters:
    ...         metric:             metric name to parse
    ...         resource_kind:      resource kind
    ...         domain:             domain name
    ...     Return parameters:
    ...         stdout    metric value
    ...     Note that to connect to login, the following variables must be set as global:
    ...         ${NODE}                     : Node where alumet is installed
    ...         ${USERNAME}                 : username to open the ssh connection
    ...         ${KEY}                      : ssh key to open the ssh connection
    [Arguments]    ${metric}    ${resource_kind}    ${domain}=${EMPTY}

    ${output}=    Read Resource Kind    ${metric}    ${domain}
    Should Contain    ${output}    ${resource_kind}

    ${output}=    Read Value    ${metric}    ${resource_kind}    ${domain}
    Should Be True    ${output} !=0.0

Install Alumet As Container
    [Documentation]    Launch Alumet as container
    ...     input parameters:
    ...         list of input plugins 
    ...     Return parameters:
    ...         none
    [Arguments]    ${list_plugins}=${EMPTY}

    Log    alumer version: ${ALUMET_VERSION}
    Log    alumer distribution: ${ALUMET_DISTRIBUTION}

    IF    '${list_plugins}' != ''
        VAR  ${entryPoint}=    '["/usr/bin/alumet-agent",  "--config", "/etc/alumet/alumet-config.toml", "--plugins", "${list_plugins}"]'

        # test if perf plugin is present
        ${perf_present}=    Evaluate    "perf" in """${list_plugins}"""

        IF    ${perf_present}
            Log    "perf" plugin is activated
            VAR  ${command}=    sudo podman run -d --pid=host --cap-add=PERFMON --cap-add=SYS_NICE --cap-add=SYS_PTRACE --entrypoint ${entryPoint} --replace --privileged --name ${ALUMET_CONTAINER_NAME} ghcr.io/alumet-dev/alumet-agent:${ALUMET_VERSION}-${ALUMET_DISTRIBUTION} watch "$(cat cpu_load.sh.pid)"
        ELSE
            Log    "perf" plugin is not activated
            VAR  ${command}=    sudo podman run -d --pid=host --cap-add=PERFMON --cap-add=SYS_NICE --cap-add=SYS_PTRACE --entrypoint ${entryPoint} --replace --privileged --name ${ALUMET_CONTAINER_NAME} ghcr.io/alumet-dev/alumet-agent:${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}
        END

    ELSE
        VAR  ${command}=    sudo podman run -d --cap-add=PERFMON --cap-add=SYS_NICE --cap-add=SYS_PTRACE --name ${ALUMET_CONTAINER_NAME} ghcr.io/alumet-dev/alumet-agent:${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}
    END

    #VAR    ${command}=    env | grep -i proxy

    ${result}=    Execute Command Target Node    ${command}
    Log    result: ${result}

UnInstall Alumet As Container
    [Documentation]    Stop and delete Alumet Container, remove temporary files used for testing

    ${result}=    Execute Command Target Node    sudo podman rm -f ${ALUMET_CONTAINER_NAME}
    Log    result: ${result}

    ${result}=    Execute Command Target Node    sudo podman ps -a | grep ${ALUMET_CONTAINER_NAME}
    Log    result: ${result}

    Should Not Contain    ${result}    ${ALUMET_CONTAINER_NAME}

    # remove alumet-output.csv file
    ${result}    ${stderr}=    Execute Command Target Node    rm alumet-output.csv
    Log    result: ${result}
    Log    stderr: ${stderr}

    ${result}    ${stderr}=    Execute Command Target Node    ls -l alumet-output.csv
    Log    result: ${result}
    Log    stderr: ${stderr}

    Should Contain    ${stderr}    cannot access

    # remove cpu_load.*
    ${result}    ${stderr}=    Execute Command Target Node    rm cpu_load.*
    Log    result: ${result}
    Log    stderr: ${stderr}

    ${result}    ${stderr}=    Execute Command Target Node    ls -l cpu_load.*
    Log    result: ${result}
    Log    stderr: ${stderr}

    Should Contain    ${stderr}    cannot access

Copy tools File
    [Documentation]    Copy tools file from Alumet container to local directory

    Open Connection    ${GATEWAY}    alias=jumphost
    Login With Public Key    ${USERNAME}    ${KEY}

    Open Connection    ${NODE}

    Login With Public Key    ${USERNAME}    ${KEY}
    ...    jumphost_index_or_alias=jumphost

    # copy tools files
    Put File    scenarios/tools/cpu_load.sh    cpu_load.sh

    Close All Connections

Copy csv File
    [Documentation]    Copy csv file from Alumet container to local directory

    ${result}=    Execute Command Target Node    sudo podman cp ${ALUMET_CONTAINER_NAME}:/app/alumet-output.csv alumet-output.csv
    Log    result: ${result}

    OperatingSystem.File Should Exist    alumet-output.csv    msg=the alumet-output.csv file does not exist
    

