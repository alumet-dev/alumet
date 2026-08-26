*** Settings ***
Documentation       Alumet installation / uninstallation

Library             OperatingSystem
Library             SSHLibrary
Resource            ../resources/alumet_keywords.resource

Suite Setup         Log    Test are running on cluster: ${NODE}    level=INFO
Test Timeout        180 seconds

Test Tags           container    installation


*** Test Cases ***
Launch Alumet Container
    [Documentation]    Launch Alumet as container

    Run Alumet Container With    csv

Stop Alumet Container
    [Documentation]    Stop and delete Alumet Container

    Stop Alumet Container
