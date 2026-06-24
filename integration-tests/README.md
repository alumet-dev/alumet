# Alumet integration tests

This folder contains all script files and scenarios for testing alumet on bare metal.

## Run the tests

Before executing the tests, you need to install robot framework. Robot framework required Python. Once python is installed,
robot framework and its dependencies are installed with the pip install command:

```bash
pip install -r requirements.txt
```

You should now be able to run the robotframework test scenarios.

## Validate the robot framework files

In order to check the lint and format of your robot files.
You must install the following tools: [robocop](https://robocop.dev/stable/)
and [robotunused](https://github.com/Lakitna/robotframework-find-unused). This can be done by running the following command:

```bash
pip install robotframework-find-unused robotframework-robocop
```

Then you should be able to use these tools to, format, lint and check your files. To do so, run:

```bash
# Lint
robocop check

# Format
robocop format

# Find any unused part, component, else
for type in arguments files keywords returns variables; do robotunused $type -v; done
```

These three commands must pass before pushing your code.
