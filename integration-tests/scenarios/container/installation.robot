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

    Install Alumet As Container    csv

Stop Alumet Container
    [Documentation]    Stop and delete Alumet Container

    UnInstall Alumet As Container
    Log    Hello
