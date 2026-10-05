*** Settings ***
Documentation       Install Alumet on k8s with rapl plugin activated

Library             OperatingSystem
Library             SSHLibrary
Resource            ../resources/alumet_keywords.resource

Suite Teardown      UnInstall Alumet As Helm Chart
Test Timeout        180 seconds

Test Tags           input_plugin    rapl_plugin    k8s


*** Test Cases ***
Install Alumet Helm Chart with rapl plugin
    [Documentation]    Install Alumet Helm Chart

    VAR    ${helm_Values}=    --set alumet-relay-client.plugins.rapl.enable="true"
    ...    --set alumet-relay-client.plugins.csv.enable="true" --set influxdb2.persistence.enabled="false"
    ...    --set alumet-relay-client.plugins.k8s.enable="false"
    Install Alumet As Helm Chart    ${helm_Values}

    # wait/retry until installation ending
    Wait Until Keyword Succeeds    1 min    10 sec    Check Alumet Helm Chart Running

Copy csv File
    [Documentation]    Copy alumet csv file

    # wait several seconds to get some metrics in csv file
    Sleep    20s

    # get the first pod name of relay-client (sed -n '1p')
    # if you want the second pod change number 1 by 2: sed -n '2p'
    VAR    ${command}=    kubectl get pods -o custom-columns=NAME:.metadata.name --no-headers |
    ...    grep ${ALUMET_CHART_INSTANCE_NAME}-alumet-relay-client | sed -n '1p'
    ${result}    ${stderr}=    Execute Command Target Node    ${command}
    Log    stderr: ${stderr}

    Copy Csv File From Pod    ${result}

Check Rapl Metric package
    [Documentation]    Check rapl_consumed_energy_J metric for cpu_package
    [Template]    Check Metric
    rapl_consumed_energy_J    cpu_package    package

Check Rapl Metric package_total
    [Documentation]    Check rapl_consumed_energy_J metric for package_total
    [Template]    Check Metric
    rapl_consumed_energy_J    local_machine    package_total
