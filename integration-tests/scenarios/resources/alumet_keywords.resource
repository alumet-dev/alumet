*** Settings ***
Documentation       This resource file contains reusable keywords for interacting with the Alumet monitoring system.
...                 It includes keywords for:
...                 - Comparing values with percentage precision
...                 - Executing commands on remote nodes via SSH
...                 - Installing and uninstalling the Alumet agent
...                 - Reading and checking metrics from Alumet's output CSV file

Library             OperatingSystem
Library             SSHLibrary


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

    IF    '${domain}' != ''
        VAR    ${command}=
        ...    grep ${metric} alumet-output.csv | awk -F ';' ' $8 == "${domain}" &&
        ...    $4 == "${resource_kind}" { OFS="|"; print $3 }' | sed -n '1p'
    ELSE
        VAR    ${command}=
        ...    grep ${metric} alumet-output.csv | awk -F ';' '
        ...    $4 == "${resource_kind}" { OFS="|"; print $3 }' | sed -n '1p'
    END

    # the metric value is on 3rd column
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
