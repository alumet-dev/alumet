*** Settings ***
Documentation       Empty file to allow the CI to pass


*** Variables ***
${GET_TRUE}     True


*** Test Cases ***
Assert True
    [Documentation]    Fake test

    Log    ${GET_TRUE}
    ${boolean_value}=    Get True

    Should Be True    ${boolean_value}


*** Keywords ***
Get True
    [Documentation]    Returns True
    RETURN    ${GET_TRUE}
