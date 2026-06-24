.PHONY: build test clean run verify

build:
	./build.sh

test:
	@echo "Running scanner unit tests..."
	@./run_tests.sh || (echo "Tests failed" && exit 1)
	@echo "Tests passed."

clean:
	rm -rf build/ test_output/
	@echo "Clean done."

run: build
	open build/TreeSize.app || true

verify: build test
	@echo "Verification build + tests complete."
