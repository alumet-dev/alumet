.PHONY: all help init test format format-check lint unused check

all: init test

help:
	@echo "Available commands:"
	@echo "  make                - Install dependencies and run tests"
	@echo "  make check          - Run format + lint + unused"
	@echo "  make init           - Install poetry dependencies"
	@echo "  make format         - Format Robot Framework files"
	@echo "  make format-check   - Check the format of Robot Framework files"
	@echo "  make lint           - Run Robocop lint"
	@echo "  make test           - Run all tests from script"
	@echo "  make unused         - Find unused Robot keywords/vars"

check: format-check lint unused

init:
	poetry install --all-groups

format:
	poetry run robocop format

format-check:
	poetry run robocop format --check

lint:
	poetry run robocop check

test:
	poetry run robot --outputdir ./output ./scenarios

unused:
	for type in arguments files keywords returns variables; do \
		poetry run robotunused $$type -v; \
	done
