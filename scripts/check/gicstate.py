"""verify pending interrupt records across signed processes and unsafe file inputs."""

from cpustate import main


if __name__ == '__main__':
  main('gicstate', 'interrupts.hgi', 16 * 1024 * 1024 + 40)
