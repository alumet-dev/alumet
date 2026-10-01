*** Settings ***
Documentation       Standard Alumet installation / uninstallation,
...                 no input plugins enabled (default helm chart configuration)

Library             OperatingSystem
Library             SSHLibrary
Resource            ../resources/alumet_keywords.resource

Suite Setup         Log    Test are running on cluster: ${NODE}    level=INFO
Test Timeout        180 seconds

Test Tags           k8s    installation


*** Test Cases ***
Install Alumet Helm Chart
    [Documentation]    Install Alumet Helm Chart

    VAR    ${helm_Values}=    --set influxdb2.persistence.enabled="false"

    Install Alumet As Helm Chart    ${helm_Values}

    # wait/retry until installation ending
    Wait Until Keyword Succeeds    1 min    10 sec    Check Alumet Helm Chart Running

Uninstall Alumet Helm Chart
    [Documentation]    Uninstall Alumet Helm Chart

    UnInstall Alumet As Helm Chart

    # wait/retry until uninstallation ending
    Wait Until Keyword Succeeds    1 min    10 sec    Check Alumet Helm Chart Not Running
