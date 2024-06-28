# Intro

This is a convoluted way to get a fancy chroot environment containing a nix install on HPCs with Apptainer. Steps are:
    
- Build a minimal Docker image containing nix
  - Do not use official NixOS image, as it is not POSIX complient and we need compatability with the HPC/host. Apptainer needs to mount various dirs (/var, /etc, /proc) to get things like userid.
- Save it as a tarball / docker-archive
- Use the archive to build a writable sandboxed Apptainer image
  - With fakeroot to allow modification of /nix
