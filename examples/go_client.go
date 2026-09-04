//go:build ignore

package main

import (
	"encoding/json"
	"fmt"
	"log"

	"github.com/Bharathcoorg/intermcp/go/intermcp"
)

func main() {
	fmt.Println("🚀 Connecting Go client to InterMCP Trust Runtime...")

	client := intermcp.NewClient("")
	if err := client.Start(); err != nil {
		log.Fatalf("Failed to start InterMCP client: %v", err)
	}
	defer client.Close()

	fmt.Println("✅ Protocol handshake successful!")

	tools, err := client.ListTools()
	if err != nil {
		log.Fatalf("Failed to list tools: %v", err)
	}

	fmt.Printf("📦 Registered tools (%d):\n", len(tools))
	for _, t := range tools {
		fmt.Printf("   • %s: %s\n", t.Name, t.Description)
	}

	fmt.Println("\n🔍 Executing system_info tool:")
	result, err := client.CallTool("system_info", map[string]interface{}{})
	if err != nil {
		log.Fatalf("Tool call failed: %v", err)
	}

	for _, item := range result.Content {
		var pretty map[string]interface{}
		if err := json.Unmarshal([]byte(item.Text), &pretty); err == nil {
			formatted, _ := json.MarshalIndent(pretty, "", "  ")
			fmt.Println(string(formatted))
		} else {
			fmt.Println(item.Text)
		}
	}

	fmt.Println("\n🎉 Done!")
}
