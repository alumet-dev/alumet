-include .env
export

.PHONY: all help init test format format-check lint unused check check-required-vars

# version of Alumet to installed
ALUMET_VERSION=0.9.4
ALUMET_DISTRIBUTION=1_amd64_ubuntu_22.04

REQUIRED_VARS := NODE GATEWAY USERNAME KEY 

all: init test

help:
	@echo "Current test set-up:"
	@echo "  ALUMET_VERSION         $(ALUMET_VERSION)"
	@echo "  ALUMET_DISTRIBUTION    $(ALUMET_DISTRIBUTION)"	
	@echo "  Target node            $(NODE)"
	@echo "  Modify the variables ALUMET_VERSION, ALUMET_DISTRIBUTION in this Makefile if you want to modify the set-up"
	@echo "  Modify the variables NODE, GATEWAY, USERNAME, KEY in the .env file if you want to modify the set-up"

	@echo "Available commands:"
	@echo "  make                		- Install dependencies and run tests"
	@echo "  make check          		- Run format + lint + unused"
	@echo "  make check-required-vars 	- Check if required variables are set"	
	@echo "  make init           		- Install poetry dependencies"
	@echo "  make format         		- Format Robot Framework files"
	@echo "  make format-check   		- Check the format of Robot Framework files"
	@echo "  make lint           		- Run Robocop lint"
	@echo "  make test           		- Run all tests from script"
	@echo "  make unused         		- Find unused Robot keywords/vars"

check: format-check lint unused

check-required-vars:
	$(foreach var,$(REQUIRED_VARS), $(if $($(var)),,$(error Variable $(var) is required)))

format:
	poetry run robocop format

format-check:
	poetry run robocop format --check

init:
	poetry install --all-groups

lint:
	poetry run robocop check

test: check-required-vars
	@echo "Current test set-up:"
	@echo "  ALUMET_VERSION         $(ALUMET_VERSION)"
	@echo "  ALUMET_DISTRIBUTION    $(ALUMET_DISTRIBUTION)"	
	@echo "  Target node            $(NODE)"
	@echo "  Modify the variables ALUMET_VERSION, ALUMET_DISTRIBUTION in this Makefile if you want to modify the set-up"
	@echo "  Modify the variables NODE, GATEWAY, USERNAME, KEY in the .env file if you want to modify the set-up"

	poetry run robot \
		--outputdir ./output \
		-v "NODE:$(NODE)"       \
        -v "GATEWAY:$(GATEWAY)" \
        -v "USERNAME:$(USERNAME)" \
        -v "KEY:$(KEY)" \
        -v "ALUMET_VERSION:$(ALUMET_VERSION)" \
        -v "ALUMET_DISTRIBUTION:$(ALUMET_DISTRIBUTION)" \
        --metadata "Alumet version":$(ALUMET_VERSION) \
		--metadata "Alumet distribution":$(ALUMET_DISTRIBUTION) \
		--metadata "Environnement target node":$(NODE) \
        ./scenarios/


unused:
	for type in arguments files keywords returns variables; do \
		poetry run robotunused $$type -v; \
	done
