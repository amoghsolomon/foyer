.PHONY: check server-check server-fmt android-test android-lint shell-check sidecar-test compose-config production-compose-config production-env stack-dev stack-down stack-health notes-dev notes-down notes-health backup-age-identity backup-test-up backup-test-down backup-create backup-list backup-restore-drill backup-roundtrip backup-test-clean backup-check backup-image

check: server-check android-test android-lint shell-check sidecar-test compose-config backup-check

server-check:
	cargo check --locked --manifest-path services/server/Cargo.toml
	cargo test --locked --manifest-path services/server/Cargo.toml

server-fmt:
	cargo fmt --manifest-path services/server/Cargo.toml -- --check

stack-dev:
	test -f deploy/.env || cp deploy/.env.example deploy/.env
	docker compose --env-file deploy/.env -f deploy/compose.yaml up --build -d
	$(MAKE) stack-health

stack-down:
	docker compose --env-file deploy/.env -f deploy/compose.yaml down

stack-health:
	@echo "Waiting for Foyer Server, PowerSync, and Radicale..."
	@for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do \
		if curl -sf http://127.0.0.1:3583/health/ready >/dev/null \
			&& curl -sf http://127.0.0.1:8080/probes/liveness >/dev/null \
			&& python3 -c "import socket; s=socket.create_connection(('127.0.0.1',5232),2); s.close()"; then \
			echo "Foyer Server: http://127.0.0.1:3583/health/ready"; \
			echo "PowerSync:    http://127.0.0.1:8080/probes/liveness"; \
			echo "Radicale:     127.0.0.1:5232 (loopback development DAV)"; \
			exit 0; \
		fi; \
		sleep 3; \
	done; \
	echo "Local personal-data stack did not become healthy in time."; \
	exit 1

notes-dev: stack-dev
notes-down: stack-down
notes-health: stack-health

android-test:
	./scripts/android-gradle :app:testDebugUnitTest

android-lint:
	./scripts/android-gradle :app:lintDebug

shell-check:
	cargo check --locked --manifest-path apps/shell/Cargo.toml --workspace

sidecar-test:
	npm --prefix apps/shell/sidecar test

compose-config: production-compose-config
	docker compose --env-file deploy/.env.example -f deploy/compose.yaml config --quiet
	docker compose --env-file deploy/.env.example -f deploy/compose.yaml --profile backup --profile backup-test config --quiet

production-compose-config:
	./deploy/scripts/validate-production-compose.sh

production-env:
	@./deploy/scripts/generate-production-env.sh

backup-age-identity:
	./deploy/backup/scripts/host.sh age-identity

backup-image:
	docker compose --env-file $(if $(wildcard deploy/.env),deploy/.env,deploy/.env.example) -f deploy/compose.yaml --profile backup build backup

backup-test-up:
	./deploy/backup/scripts/host.sh test-up

backup-test-down:
	./deploy/backup/scripts/host.sh test-down

backup-create:
	./deploy/backup/scripts/host.sh create

backup-list:
	./deploy/backup/scripts/host.sh list

backup-restore-drill:
	./deploy/backup/scripts/host.sh restore-drill

backup-roundtrip:
	./deploy/backup/tests/roundtrip.sh

backup-test-clean:
	./deploy/backup/scripts/host.sh clean-test

backup-check:
	./deploy/backup/tests/static.sh
	./deploy/backup/tests/unit.sh
	python3 -m py_compile deploy/backup/scripts/s3.py deploy/backup/tests/compare.py
