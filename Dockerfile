FROM rockylinux:9.3-minimal

RUN curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install linux \
		--extra-conf "sandbox = false" \
		--init none \
		--no-confirm

ENV PATH="${PATH}:/nix/var/nix/profiles/default/bin"
