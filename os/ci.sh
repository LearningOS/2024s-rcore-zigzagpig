#!/bin/sh

test() {
  # Sample input line (in practice, replace this with actual command output)
  # output="Test passed21919: 15/15"
  output=$(make test CHAPTER=4 OFFLINE=1)
  echo "$output"

  # Extract the part after "Test passed" and before the colon
  # Use `awk` to split and retrieve numbers
  result=$(echo "$output" | awk -F ': ' '{print $2}')

  # Extract the passed and total numbers
  passed=$(echo "$result" | awk -F '/' '{print $1}')
  total=$(echo "$result" | awk -F '/' '{print $2}')

  # Check if the passed number is equal to the total number
  if [ "$passed" -eq "$total" ]; then
    echo "All tests passed."
  else
    echo "Some tests failed."
  fi
}

clone_repos() {

  # 定义临时仓库存放位置
  REPO_DIR=~/2024s-rcore-zigzagpig/repos

  # 定义目标仓库克隆位置
  TARGET_DIR=~/2024s-rcore-zigzagpig/ci-user

  # 检查临时仓库是否已经存在，如果不存在则克隆
  [ ! -d "$REPO_DIR/rCore-Tutorial-Checker-2024S" ] && git clone https://github.com/LearningOS/rCore-Tutorial-Checker-2024S.git "$REPO_DIR/rCore-Tutorial-Checker-2024S"
  [ ! -d "$REPO_DIR/rCore-Tutorial-Test-2024S" ] && git clone https://github.com/LearningOS/rCore-Tutorial-Test-2024S.git "$REPO_DIR/rCore-Tutorial-Test-2024S"

  # 创建目标目录
  mkdir -p "$TARGET_DIR"
  mkdir -p "$TARGET_DIR/user"

  # 从临时仓库克隆到目标位置
  git clone "$REPO_DIR/rCore-Tutorial-Checker-2024S" "$TARGET_DIR"
  git clone "$REPO_DIR/rCore-Tutorial-Test-2024S" "$TARGET_DIR/user"

}

# Define the commands to execute in a function
execute_commands() {
  echo "Executing commands..."
  # Place your commands here
  # For example:
  cd ~/2024s-rcore-zigzagpig/ || exit
  sudo rm -r ~/2024s-rcore-zigzagpig/ci-user
  git reset --hard HEAD
  # git clone https://github.com/LearningOS/rCore-Tutorial-Checker-2024S.git ci-user
  # git clone https://github.com/LearningOS/rCore-Tutorial-Test-2024S.git ci-user/user
  clone_repos
  cd ~/2024s-rcore-zigzagpig/ci-user || exit
  test
  git reset --hard HEAD
  git status
  # Add more commands as needed
}

# Capture the output of git status
output=$(git status)

# Define the lines to check
line1="config.toml"
line2="Makefile"
line3="build.rs"
clean_message="nothing to commit, working tree clean"

# Check if all the lines are present in the output
if echo "$output" | grep -q "$line1" && echo "$output" | grep -q "$line2" && echo "$output" | grep -q "$line3"; then
  echo "All specified lines found. Executing commands."
  execute_commands
# Check if the clean message is present in the output
elif echo "$output" | grep -q "$clean_message"; then
  echo "Working tree clean. Executing commands."
  execute_commands
else
  echo "Conditions not met. No action taken."
fi
