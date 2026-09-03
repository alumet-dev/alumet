*** Settings ***
Documentation       Install Alumet on k8s with rapl plugin activated

Library             OperatingSystem
Library             SSHLibrary
Resource            ../resources/alumet_keywords.resource

Suite Setup         Log    Test are running on cluster: ${NODE}    level=INFO
Test Timeout        180 seconds

Test Tags           k8s    installation


*** Test Cases ***
Install Alumet Helm Chart with rapl plugin
    [Documentation]    Install Alumet Helm Chart

    VAR    ${helm_Values}=    --set alumet-relay-client.plugins.rapl.enable="true"
    ...    --set alumet-relay-client.plugins.csv.enable="true" --set influxdb2.persistence.enabled="false"
    Install Alumet As Helm Chart    ${helm_Values}

    # wait few seconds installation ending
    Sleep    20s

    VAR    ${command}=    kubectl get pod | grep Running
    ${result}    ${stderr}=    Execute Command Target Node    ${command}
    Log    stderr: ${stderr}
    # check relay client is running
    Should Contain    ${result}    alumet-robot-fm-alumet-relay-client
    # test relay server is running
    Should Contain    ${result}    alumet-robot-fm-alumet-relay-server
    # test influxdb is running
    Should Contain    ${result}    alumet-robot-fm-influxdb2

Copy csv File
    [Documentation]    Copy alumet csv file

    # wait several seconds to get some metrics in csv file
    Sleep    10s

    # get the first pod name of relay-client
    VAR    ${command}=    kubectl get pods -o custom-columns=NAME:.metadata.name --no-headers |
    ...    grep alumet-robot-fm-alumet-relay-client | sed -n '1p'
    ${result}    ${stderr}=    Execute Command Target Node    ${command}
    Log    stderr: ${stderr}

    Copy Csv File From Pod    ${result}

Check Rapl Metric package
    [Documentation]    Check rapl_consumed_energy_J metric for cpu_package
    [Template]    Check Metric
    # ${metric}    ${resource_kind}    ${domain}    ${installation_type}
    rapl_consumed_energy_J    cpu_package    package    k8s

Check Rapl Metric package_total
    [Documentation]    Check rapl_consumed_energy_J metric for package_total
    [Template]    Check Metric
    # ${metric}    ${resource_kind}    ${domain}    ${installation_type}
    rapl_consumed_energy_J    local_machine    package_total    k8s

Uninstall Alumet Helm Chart
    [Documentation]    Uninstall Alumet Helm Chart

    UnInstall Alumet As Helm Chart

    # wait few seconds installation ending
    Sleep    30s

    # check relay client is running
    VAR    ${command}=    kubectl get pod
    ${result}    ${stderr}=    Execute Command Target Node    ${command}
    Log    stderr: ${stderr}
    Should Not Contain    ${result}    alumet-robot-fm-alumet-relay-client
    # test relay server is running
    Should Not Contain    ${result}    alumet-robot-fm-alumet-relay-server
    # test influxdb is running
    Should Not Contain    ${result}    alumet-robot-fm-influxdb2
