-include .env
export

.PHONY: all help init test format format-check lint unused check check-required-vars

# version of Alumet to installed
ALUMET_VERSION=0.9.4
# distribution of Alumet to installed
# Note that all versions are not available as docker images; for example version 22.04 is not available for ubuntu, but 24.04 is.
# For this version of integration test, we test only the OCI images.
ALUMET_DISTRIBUTION=ubuntu_24.04
ALUMET_ARCHITECTURE=amd64

# For k8s tests
ALUMET_CHART_VERSION=0.8.2

REQUIRED_VARS := NODE GATEWAY USERNAME KEY 

all: init test

help:
	@echo "Current test set-up:"
	@echo "  ALUMET_VERSION         	$(ALUMET_VERSION)"
	@echo "  ALUMET_DISTRIBUTION    	$(ALUMET_DISTRIBUTION)"
	@echo "  ALUMET_ARCHITECTURE    	$(ALUMET_ARCHITECTURE)"
	@echo "  ALUMET_CHART_VERSION    	$(ALUMET_CHART_VERSION)"
	@echo "  Target node            	$(NODE)"
	@echo "  Modify the variables ALUMET_VERSION, ALUMET_DISTRIBUTION, ALUMET_ARCHITECTURE in this Makefile if you want to modify the set-up"
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
	@echo "  ALUMET_VERSION         	$(ALUMET_VERSION)"
	@echo "  ALUMET_DISTRIBUTION    	$(ALUMET_DISTRIBUTION)"	
	@echo "  ALUMET_ARCHITECTURE    	$(ALUMET_ARCHITECTURE)"	
	@echo "  ALUMET_CHART_VERSION    	$(ALUMET_CHART_VERSION)"	
	@echo "  Target node            	$(NODE)"
	@echo "  Modify the variables ALUMET_VERSION, ALUMET_DISTRIBUTION, ALUMET_ARCHITECTURE in this Makefile if you want to modify the set-up"
	@echo "  Modify the variables NODE, GATEWAY, USERNAME, KEY in the .env file if you want to modify the set-up"

	# Note: to exclude some tests you can:
	# specify a path to a file or a directory /subdirectory (./scenario/container for example)
	# or add the option --exclude {tag name} to robot command

	poetry run robot \
		--outputdir ./output \
		-v "NODE:$(NODE)"       \
        -v "GATEWAY:$(GATEWAY)" \
        -v "USERNAME:$(USERNAME)" \
        -v "KEY:$(KEY)" \
        -v "ALUMET_VERSION:$(ALUMET_VERSION)" \
        -v "ALUMET_DISTRIBUTION:$(ALUMET_DISTRIBUTION)" \
		-v "ALUMET_ARCHITECTURE:$(ALUMET_ARCHITECTURE)" \
		-v "ALUMET_CHART_VERSION:$(ALUMET_CHART_VERSION)" \
        --metadata "Alumet version":$(ALUMET_VERSION) \
		--metadata "Alumet distribution":$(ALUMET_DISTRIBUTION) \
		--metadata "Alumet architecture":$(ALUMET_ARCHITECTURE) \
		--metadata "Alumet chart version":$(ALUMET_CHART_VERSION) \
		--metadata "Environnement target node":$(NODE) \
		./scenarios/

unused:
	for type in arguments files keywords returns variables; do \
		echo "Checking presence of unused $$type" ;\
		poetry run robotunused $$type -v; \
	done
