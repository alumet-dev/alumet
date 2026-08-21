*** Settings ***
Documentation       Alumet installation / uninstallation

Library             OperatingSystem
Library             SSHLibrary
Resource            ../resources/alumet_keywords.resource

Suite Setup         Log    Test are running on cluster: ${NODE}    level=INFO
Test Timeout        180 seconds

Test Tags           k8s     installation


*** Test Cases ***
Install Alumet Helm Chart
    [Documentation]    Install Alumet Helm Chart

    Install Alumet As Helm Chart

    # wait few seconds installation ending
    Sleep   20s

    VAR    ${command}=      kubectl get pod | grep Running
    ${result}   ${stderr}=    Execute Command Target Node    ${command}

    # check relay client is running
    Should Contain    ${result}     alumet-robot-fm-alumet-relay-client
    # test relay server is running
    Should Contain    ${result}     alumet-robot-fm-alumet-relay-server
    # test influxdb is running
    Should Contain    ${result}     alumet-robot-fm-influxdb2

Uninstall Alumet Helm Chart
    [Documentation]    Uninstall Alumet Helm Chart

    UnInstall Alumet As Helm Chart

    # wait few seconds installation ending
    Sleep   30s

    # check relay client is running
    VAR    ${command}=      kubectl get pod
    ${result}   ${stderr}=    Execute Command Target Node    ${command}
    Should Not Contain    ${result}     alumet-robot-fm-alumet-relay-client
    # test relay server is running
    Should Not Contain    ${result}     alumet-robot-fm-alumet-relay-server
    # test influxdb is running
    Should Not Contain    ${result}     alumet-robot-fm-influxdb2