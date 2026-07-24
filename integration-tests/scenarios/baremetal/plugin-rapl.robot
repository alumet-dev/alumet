*** Settings ***
Documentation       Alumet test plugin rapl

Library             OperatingSystem
Library             SSHLibrary
Resource            ../resources/alumet_keywords.resource

Test Timeout        60 seconds

Test Tags           input_plugin    rapl_plugin


*** Test Cases ***
Test connection on target node
    [Documentation]    Verify SSH connection to the target node

    ${output}    ${stderr}=    Execute Command Target Node    hostname
    Log    Output Result of SSH : ${output}
    Log    stderr Result of SSH : ${stderr}

Run plugins socket-control csv rapl
    [Documentation]    Run alumet-agent with csv, rapl and socket-control plugins

    ${output}    ${stderr}=    Execute Command Target Node
    ...    alumet-agent --plugins csv,rapl,socket-control > /tmp/alumet.log 2>&1 &
    Sleep    3s
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    ${output_alumet}    ${stderr}=    Execute Command Target Node    cat /tmp/alumet.log
    Log    Result stdout : ${output_alumet}
    Log    stderr Result : ${stderr}

    Should Contain    ${output_alumet}    3 plugins started

Check plugins socket-control csv rapl
    [Documentation]    Check plugins socket-control csv rapl

    ${output_alumet}    ${stderr}=    Execute Command Target Node    cat /tmp/alumet.log
    Log    Result stdout : ${output_alumet}
    Log    stderr Result : ${stderr}

    Should Contain    ${output_alumet}    csv v0.2.0
    Should Contain    ${output_alumet}    socket-control v0.2.1
    Should Contain    ${output_alumet}    rapl v0.3.1

Check alumet running
    [Documentation]    Verify that alumet-agent is running with the correct plugins

    ${output}    ${stderr}=    Execute Command Target Node    ls alumet-control.sock
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    Should Contain    ${output}    alumet-control.sock

    ${output}    ${stderr}=    Execute Command Target Node    ps -f -u ${USERNAME}
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    Should Contain    ${output}    /usr/lib/alumet-agent --plugins csv,rapl,socket-control

    ${result}=    Compare Values Percent    100    105    8
    Should Be True    ${result}

Check Rapl Metric package
    [Documentation]    Check rapl_consumed_energy_J metric for cpu_package
    [Template]    Check Metric
    rapl_consumed_energy_J    cpu_package    package

Check Rapl Metric package_total
    [Documentation]    Check rapl_consumed_energy_J metric for package_total
    [Template]    Check Metric
    rapl_consumed_energy_J    local_machine    package_total

Check Rapl Metric dram
    [Documentation]    Check rapl_consumed_energy_J metric for dram
    [Template]    Check Metric
    rapl_consumed_energy_J    dram    dram

Check Rapl Metric dram_total
    [Documentation]    Check rapl_consumed_energy_J metric for dram_total
    [Template]    Check Metric
    rapl_consumed_energy_J    local_machine    dram_total

Stop alumet
    [Documentation]    Stop alumet-agent using socket control

    ${output}    ${stderr}=    Execute Command Target Node
    ...    echo "shutdown" | socat UNIX-CONNECT:alumet-control.sock -
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    ${output}    ${stderr}=    Execute Command Target Node    ls alumet-control.sock
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    Should Not Contain    ${output}    alumet-control.sock

Check alumet not running
    [Documentation]    Verify that alumet-agent is not running
    ${output}    ${stderr}=    Execute Command Target Node
    ...    echo "shutdown" | socat UNIX-CONNECT:alumet-control.sock -
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    ${output}    ${stderr}=    Execute Command Target Node    ps -f -u ${USERNAME}
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    Should Not Contain    ${output}    alumet-agent

Check alumet-control file
    [Documentation]    Verify that alumet-control.sock file is not present

    ${output}    ${stderr}=    Execute Command Target Node    ls alumet-control.sock
    Log    Result stdout : ${output}
    Log    stderr Result : ${stderr}

    Should Not Contain    ${output}    alumet-control.sock
