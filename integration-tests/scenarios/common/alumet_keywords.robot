*** Settings ***
Library    OperatingSystem
Library    SSHLibrary

*** Variables ***


*** Keywords ***

########################################################################################################
# Compare values Percent
#   Compare 2 values with a precision expressed as a % 
#   input parameters:
#       value1
#       value2
#       precision in %
#
#   return parameter:
#    boolean: true if value1 = value2 according the precision
#
########################################################################################################
Compare Values Percent
    [Arguments]     ${value1}    ${value2}    ${percent}
    
    ${diff}=    Evaluate    abs(${value1}-${value2})/${value1}*100
    ${result}=    Evaluate     ${diff} < ${percent}

    [Return]    ${result}
########################################################################################################
# Execute Command Target Node
#   Connect to a target node and execute a command on this node
#   input parameters:
#       Command:     the command to execute on remote host
#
#   Return parameters:
#        stdout    of the command executed
#                   
# Note that to connect to login, the following variables must be set as global:
#       ${NODE}                     : Node where alumet is installed
#       ${USERNAME}                 : username to open the ssh connection
#       ${KEY}                      : ssh key to open the ssh connection       
#
########################################################################################################
Execute Command Target Node
    [Arguments]     ${Command}

    Open Connection     ${GATEWAY}    alias=jumphost
    Login With Public Key             ${USERNAME}     ${KEY}

    Open Connection    ${NODE}
    
    Login With Public Key    ${USERNAME}     ${KEY}
    ...    jumphost_index_or_alias=jumphost


    ${stdout}   ${stderr}   ${rc}=    Execute Command    ${Command}
    ...     timeout=30s
    ...     return_stdout=True
    ...     return_stderr=True
    ...     return_rc=True
    # ...     output_during_execution=True    # to get more debug information uncomment this line

    Log    Result stdout : ${stdout}
    Log    Result stderr : ${stderr}
    Log    Result return code : ${rc}


    Close All Connections

    [Return]    ${stdout}    ${stderr}

########################################################################################################
# Install Alumet
#   Connect to a target node and install alumet
#   input parameters:
#       None
#
#   Return parameters:
#        stdout    of the command executed
#                   
# Note that to connect to login, the following variables must be set as global:
#       ${NODE}                     : Node where alumet is installed
#       ${USERNAME}                 : username to open the ssh connection
#       ${KEY}                      : ssh key to open the ssh connection       
#       ${ALUMET_VERSION}           : alumet version
#       ${ALUMET_DISTRIBUTION}      : alumet distribution
#
########################################################################################################
Install Alumet

    # first download the right linux package file, exit test suite if download error
    ${output}=    Run     wget https://github.com/alumet-dev/alumet/releases/download/v${ALUMET_VERSION}/alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb
    ${exists}=    Run Keyword And Return Status    OperatingSystem.File Should Exist    alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb
    Run Keyword If    ${exists}==False    Fail    'Error downloading alumet package file. Test suite is stopped'


    Open Connection     ${GATEWAY}    alias=jumphost
    Login With Public Key             ${USERNAME}     ${KEY}

    Open Connection    ${NODE}
    
    Login With Public Key    ${USERNAME}     ${KEY}
    ...    jumphost_index_or_alias=jumphost

    # create tmp directory to put all required files on target node

    # copy linux package on remote host
    Put File    alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb    alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb

    # copy tools files
    Put File    scenarios/tools/cpu_load.sh     cpu_load.sh

    # install alumet package
    ${output}    ${stderr}=    Execute Command Target Node    sudo DEBIAN_FRONTEND=noninteractive apt install -y ./alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb

    # check il installation ok
    ${result}    ${stderr}=    Execute Command Target Node    apt list --installed alumet-agent

    # cancel test suite if installation failed
    ${exists}=    Run Keyword And Return Status     Should Contain    ${result}    alumet
    Run Keyword If    ${exists}==False    Fail    'Error installing alumet. Test suite is stopped'

    ${exists}=    Run Keyword And Return Status    should Contain    ${result}    ${ALUMET_VERSION}
    Run Keyword If    ${exists}==False    Fail    'Error installing alumet. Test suite is stopped'

    Close All Connections

    [Return]    ${output}

########################################################################################################
# UnInstall Alumet
#   Connect to a target node and uninstall alumet
#   input parameters:
#       None
#
#   Return parameters:
#        stdout    of the command executed
#                   
# Note that to connect to login, the following variables must be set as global:
#       ${NODE}                     : Node where alumet is installed
#       ${USERNAME}                 : username to open the ssh connection
#       ${KEY}                      : ssh key to open the ssh connection       
#       ${ALUMET_VERSION}           : alumet version
#       ${ALUMET_DISTRIBUTION}      : alumet distribution
#
########################################################################################################
UnInstall Alumet

    ${output}    ${stderr}=    Execute Command Target Node    sudo DEBIAN_FRONTEND=noninteractive apt purge -y alumet-agent

    ${result}    ${stderr}=    Execute Command Target Node    apt list --installed alumet-agent

    Should Not Contain    ${result}    alumet

    # remove alumet-output.csv file
    ${result}    ${stderr}=    Execute Command Target Node    rm alumet-output.csv

    # remove alumet package file on target node
    ${result}    ${stderr}=    Execute Command Target Node    rm alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb*

    ${result}    ${stderr}=    Execute Command Target Node    ls -l alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb*

    Should Not Contain    ${result}    alumet

    # remove alumet package file downloaded locally
    ${result}=    Run     rm alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb*
    ${result}=    Run     ls -l alumet-agent_${ALUMET_VERSION}-${ALUMET_DISTRIBUTION}.deb*
    Should Contain    ${result}    cannot access

    # remove cpu_load.*
    ${result}    ${stderr}=    Execute Command Target Node    rm cpu_load.*
    ${result}    ${stderr}=    Execute Command Target Node    ls -l cpu_load.*
    Should Contain    ${stderr}    cannot access


    [Return]    ${output}

########################################################################################################
# Read resource_kind column
#   Connect to a target node and read the column  resource_kind of alumet-output.csv file.
#   input parameters:
#         metric:    metric name to parse   
#         domain: domain name  
#
#   Return parameters:
#        stdout    of the command executed
#                   
# Note that to connect to login, the following variables must be set as global:
#       ${NODE}                     : Node where alumet is installed
#       ${USERNAME}                 : username to open the ssh connection
#       ${KEY}                      : ssh key to open the ssh connection       
#
########################################################################################################
Read resource_kind
    [Arguments]     ${metric}    ${domain}=${EMPTY}

    # the metric resource_kind is on 4th column 
    IF    '${domain}' != ''
        ${command}=   Set variable     grep ${metric} alumet-output.csv | awk -F ';' ' $8 == "${domain}" { OFS="|"; print $4 }'
    ELSE
        ${command}=   Set variable     grep ${metric} alumet-output.csv
    END

    ${output}    ${stderr}=    Execute Command Target Node     ${command}

    [Return]    ${output}

########################################################################################################
# Read value column
#   Connect to a target node and read the metric value on the first line of alumet-output.csv file.
#   input parameters:
#         metric: metric name to parse 
#         resource_kind: resource kind name
#         domain: domain name  
#
#   Return parameters:
#        stdout    metric value
#                   
# Note that to connect to login, the following variables must be set as global:
#       ${NODE}                     : Node where alumet is installed
#       ${USERNAME}                 : username to open the ssh connection
#       ${KEY}                      : ssh key to open the ssh connection       
#
########################################################################################################
Read value
    [Arguments]     ${metric}    ${resource_kind}    ${domain}=${EMPTY}

    IF    '${domain}' != ''
        ${command}=   Set variable     grep ${metric} alumet-output.csv | awk -F ';' ' $8 == "${domain}" && $4 == "${resource_kind}" { OFS="|"; print $3 }' | sed -n '1p'
    ELSE
        ${command}=   Set variable     grep ${metric} alumet-output.csv | awk -F ';' ' $4 == "${resource_kind}" { OFS="|"; print $3 }' | sed -n '1p'
    END


    # the metric value is on 3rd column 
    ${output}    ${stderr}=    Execute Command Target Node     ${command} 
    Log To Console     metric value read: ${output}

    [Return]    ${output}

########################################################################################################
# check Metric
#   Connect to a target node and check metric using the 2 keywords:
#     Read resource_kind   
#     Read Value
#
#   input parameters:
#         metric:             metric name to parse
#         resource_kind:      resource kind
#         domain:             domain name  
#
#   Return parameters:
#        stdout    metric value
#                   
# Note that to connect to login, the following variables must be set as global:
#       ${NODE}                     : Node where alumet is installed
#       ${USERNAME}                 : username to open the ssh connection
#       ${KEY}                      : ssh key to open the ssh connection       
#
########################################################################################################
Check Metric
    [Arguments]     ${metric}    ${resource_kind}    ${domain}=${EMPTY}

    ${output}=     Read resource_kind    ${metric}    ${domain}
    Should Contain     ${output}    ${resource_kind}

    ${output}=    Read value            ${metric}    ${resource_kind}    ${domain}
    Should Be True    ${output} !=0.0