DOCKER_TAG ?= elfloader:latest
.PHONY: docker build_docker

current_dir := $(shell pwd)

docker:
	docker run --rm -it -v ${current_dir}:/mnt -w /mnt --name elfloader ${DOCKER_TAG} bash

build_docker: 
	docker build -t ${DOCKER_TAG} --progress=plain .