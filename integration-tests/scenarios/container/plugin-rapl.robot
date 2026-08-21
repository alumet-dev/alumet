*** Settings ***
Documentation       Alumet test plugin rapl

Library             OperatingSystem
Library             String
Library             SSHLibrary
Resource            ../resources/alumet_keywords.resource

Test Timeout        60 seconds

Test Tags           container    input_plugin    rapl_plugin


*** Test Cases ***
Test connection on target node
    [Documentation]    Verify SSH connection to the target node

    ${output}    ${stderr}=    Execute Command Target Node    hostname
    Log    Output Result of SSH : ${output}
    Log    stderr Result of SSH : ${stderr}

Run plugins csv rapl
    [Documentation]    Run alumet-agent with csv and rapl plugins

    Install Alumet As Container    csv,rapl

    ${result}    ${stderr}=    Execute Command Target Node    sudo podman logs ${ALUMET_CONTAINER_NAME}
    Log    result: ${result}
    Log    stderr: ${stderr}

    # Note that the output of podman log command is redirected to stderr
    Should Contain    ${stderr}    Starting Alumet
    Should Contain    ${stderr}    ${ALUMET_VERSION}

    # check that csv and perf plugins are started
    ${started_section}=    Get Regexp Matches
    ...    ${stderr}
    ...    plugins started:(.*?)plugins disabled:
    ...    1
    ...    flags=DOTALL
    Should Contain    ${started_section}[0]    csv
    Should Contain    ${started_section}[0]    rapl

Check alumet running
    [Documentation]    Verify that alumet-agent is running with the correct plugins

    ${output}    ${stderr}=    Execute Command Target Node    sudo podman exec ${ALUMET_CONTAINER_NAME} ps -f
    Log    Result stdout : ${output}
    Log    Result stderr : ${stderr}

    # Note that the output of podman log command is redirected to stderr
    Should Contain    ${output}    /usr/bin/alumet-agent
    Should Contain    ${output}    --plugins csv,rapl

Copy csv File
    [Documentation]    Copy alumet csv file

    # wait several seconds to get some metrics in csv file
    Sleep    10s
    Copy Csv File

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
    [Documentation]    Stop alumet-agent delete alumet container

    UnInstall Alumet As Container
    Log    Stop alumet

Check alumet not running
    [Documentation]    Verify that alumet-agent is not running

    ${output}=    Execute Command Target Node    sudo podman exec ${ALUMET_CONTAINER_NAME} ps -f
    Log    Result stdout : ${output}

    Should Not Contain    ${output}    alumet-agent
