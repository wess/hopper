"""verify sparse memory records across signed processes and unsafe file inputs."""

from cpustate import main


if __name__ == '__main__':
  main('ramstate', 'memory.hrm', (32 << 20) + ((32 << 20) // 65536) * 4 + 56)
